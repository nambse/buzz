#!/usr/bin/env python3
"""Validate structural discovery for ignored PostgreSQL-backed Rust tests."""

from __future__ import annotations

import re
import sys
from collections import deque
from dataclasses import dataclass
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9 on supported macOS development hosts.
    tomllib = None

IGNORE_ATTRIBUTE = re.compile(r"#\s*\[\s*ignore\s*=")
BARE_IGNORE_ATTRIBUTE = re.compile(r"#\s*\[\s*ignore\s*\]")
FUNCTION = re.compile(r"\b(?:async\s+)?fn\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)")
MODULE = re.compile(r"\bmod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\{")
OUT_OF_LINE_MODULE = re.compile(
    r"(?P<attributes>(?:#\s*\[[^]]*\]\s*)*)"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?\bmod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*;"
)
PATH_ATTRIBUTE = re.compile(r"#\s*\[\s*path\s*=\s*")
EXTERNAL_INFRA = re.compile(r"\b(?:s3|minio|storage|docker|network)\b", re.IGNORECASE)
RAW_STRING = re.compile(r'(?:b?r)(?P<hashes>#{0,255})"')
CHAR_LITERAL = re.compile(r"(?:b)?'(?:\\(?:u\{[0-9A-Fa-f_]+\}|x[0-9A-Fa-f]{2}|.)|[^\\'\n])'")
TOML_SECTION = re.compile(r"^\s*\[(?P<name>[^]]+)]\s*(?:#.*)?$")
TOML_PACKAGE_NAME = re.compile(
    r"^\s*name\s*=\s*(?P<quote>['\"])(?P<name>[A-Za-z0-9_-]+)(?P=quote)\s*(?:#.*)?$"
)


def sanitize_rust(source: str) -> str:
    """Blank comments and literals while preserving byte offsets and braces."""
    chars = list(source)
    index = 0
    length = len(source)
    while index < length:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            end = length if end == -1 else end
            for offset in range(index, end):
                chars[offset] = " "
            index = end
            continue
        if source.startswith("/*", index):
            start = index
            depth = 1
            index += 2
            while index < length and depth:
                if source.startswith("/*", index):
                    depth += 1
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            for offset in range(start, index):
                if chars[offset] != "\n":
                    chars[offset] = " "
            continue

        character = CHAR_LITERAL.match(source, index)
        if character:
            start = index
            index = character.end()
            for offset in range(start, index):
                chars[offset] = " "
            continue

        raw = RAW_STRING.match(source, index)
        if raw:
            start = index
            terminator = '"' + raw.group("hashes")
            index = raw.end()
            end = source.find(terminator, index)
            index = length if end == -1 else end + len(terminator)
            for offset in range(start, index):
                if chars[offset] != "\n":
                    chars[offset] = " "
            continue

        quote_start = index
        if source.startswith('b"', index):
            index += 1
        if source[index] == '"':
            index += 1
            while index < length:
                if source[index] == "\\":
                    index += 2
                elif source[index] == '"':
                    index += 1
                    break
                else:
                    index += 1
            for offset in range(quote_start, min(index, length)):
                if chars[offset] != "\n":
                    chars[offset] = " "
            continue
        index += 1
    return "".join(chars)


def parse_rust_string_literal(source: str, start: int) -> tuple[str, int] | None:
    """Parse an ordinary or raw Rust string literal at or after start."""
    index = start
    while index < len(source) and source[index].isspace():
        index += 1

    raw = RAW_STRING.match(source, index)
    if raw:
        content_start = raw.end()
        terminator = '"' + raw.group("hashes")
        content_end = source.find(terminator, content_start)
        if content_end == -1:
            return None
        return source[content_start:content_end], content_end + len(terminator)

    if index >= len(source) or source[index] != '"':
        return None
    index += 1
    content = []
    while index < len(source):
        if source[index] == "\\":
            if index + 1 >= len(source):
                return None
            content.append(source[index + 1])
            index += 2
        elif source[index] == '"':
            return "".join(content), index + 1
        else:
            content.append(source[index])
            index += 1
    return None


def ignore_attributes(source: str, sanitized: str) -> list[tuple[int, int, str]]:
    """Return real ignore attributes and reasons, excluding comments."""
    attributes = []
    for match in IGNORE_ATTRIBUTE.finditer(sanitized):
        parsed = parse_rust_string_literal(source, match.end())
        if parsed is None:
            continue
        reason, literal_end = parsed
        attribute_end = literal_end
        while attribute_end < len(source) and source[attribute_end].isspace():
            attribute_end += 1
        if attribute_end >= len(source) or source[attribute_end] != "]":
            continue
        attributes.append((match.start(), attribute_end + 1, reason))
    return attributes


def module_ranges(source: str) -> list[tuple[int, int, str]]:
    sanitized = sanitize_rust(source)
    brace_pairs: dict[int, int] = {}
    stack: list[int] = []
    for index, char in enumerate(sanitized):
        if char == "{":
            stack.append(index)
        elif char == "}" and stack:
            brace_pairs[stack.pop()] = index

    ranges = []
    for match in MODULE.finditer(sanitized):
        open_brace = sanitized.find("{", match.start(), match.end())
        close_brace = brace_pairs.get(open_brace)
        if close_brace is not None:
            ranges.append((open_brace, close_brace, match.group("name")))
    return ranges


def crate_root(path: Path) -> Path | None:
    for candidate in path.parents:
        if (candidate / "Cargo.toml").is_file():
            return candidate
    return None


def integration_binary_is_postgres(path: Path) -> bool:
    root = crate_root(path)
    return (
        root is not None
        and (
            (path.parent == root / "tests" and path.name.startswith("postgres_"))
            or (
                path.name == "main.rs"
                and path.parent.parent == root / "tests"
                and path.parent.name.startswith("postgres_")
            )
        )
    )


def standard_module_paths(
    parent_source: Path, module_name: str, inline_modules: tuple[str, ...] = ()
) -> list[Path]:
    """Resolve Rust's conventional out-of-line module source paths."""
    root = crate_root(parent_source)
    is_crate_root = parent_source.name in {"lib.rs", "main.rs", "mod.rs"}
    if root is not None:
        is_crate_root = is_crate_root or (
            parent_source.parent == root / "tests"
            or parent_source.parent == root / "examples"
            or parent_source.parent == root / "benches"
            or parent_source.parent == root / "src" / "bin"
        )
    base = (
        parent_source.parent
        if is_crate_root
        else parent_source.parent / parent_source.stem
    )
    base = base.joinpath(*inline_modules)
    return [base / f"{module_name}.rs", base / module_name / "mod.rs"]


@dataclass(frozen=True)
class DiscoveryContext:
    """The two namespace predicates used by the actual nextest profile."""

    postgres: bool = False
    external: bool = False

    def under(self, modules: tuple[str, ...]) -> DiscoveryContext:
        return DiscoveryContext(
            self.postgres or any(name.endswith("postgres_tests") for name in modules),
            self.external or any(name.startswith("external_infra") for name in modules),
        )


def out_of_line_module_index(files: list[Path]) -> dict[Path, set[DiscoveryContext]]:
    """Carry binary and module namespace predicates through every module edge.

    A file can be included by multiple binaries or namespaces. Keep those
    contexts separate: a PostgreSQL path beneath external_infra must not lend
    its PostgreSQL flag to an unrelated ordinary path. The four possible
    contexts bound traversal even for cyclic or multiply included sources.
    """
    context_files = {path.resolve() for path in files}
    roots = {root for path in context_files if (root := crate_root(path)) is not None}
    for root in roots:
        context_files.update(path.resolve() for path in root.rglob("*.rs") if "target" not in path.parts)
    for directory in {path.parent for path in files}:
        context_files.update(path.resolve() for path in directory.glob("*.rs"))
    edges: dict[Path, list[tuple[Path, tuple[str, ...]]]] = {}
    children: set[Path] = set()
    for parent_source in sorted(context_files):
        source = parent_source.read_text(encoding="utf-8")
        sanitized = sanitize_rust(source)

        ranges = module_ranges(source)
        for module in OUT_OF_LINE_MODULE.finditer(sanitized):
            module_name = module.group("name")
            inline_modules = tuple(name for start, end, name in ranges if start < module.start() < end)
            attribute = PATH_ATTRIBUTE.search(sanitized, module.start("attributes"), module.end("attributes"))
            if attribute is not None:
                equals = source.find("=", attribute.start(), attribute.end())
                parsed = parse_rust_string_literal(source, equals + 1)
                if parsed is None:
                    continue
                # In an inline module, Rust resolves #[path] from the inline
                # module directory; without one it uses the containing file.
                base = parent_source.parent
                if inline_modules:
                    base = standard_module_paths(parent_source, "unused", inline_modules)[0].parent
                candidates = [base / parsed[0]]
            else:
                candidates = standard_module_paths(parent_source, module_name, inline_modules)
            for candidate in candidates:
                child = candidate.resolve()
                if child in context_files:
                    edges.setdefault(parent_source, []).append((child, inline_modules + (module_name,)))
                    children.add(child)

    contexts: dict[Path, set[DiscoveryContext]] = {}
    pending: deque[tuple[Path, DiscoveryContext]] = deque()
    for path in sorted(context_files):
        # Unreferenced files retain standalone structural validation. Actual
        # Cargo roots must also be seeded when another module includes them.
        root = crate_root(path)
        cargo_root = root is not None and (
            path in {root / "src" / "lib.rs", root / "src" / "main.rs"}
            or path.parent == root / "tests"
            or (path.name == "main.rs" and path.parent.parent == root / "tests")
        )
        if path not in children or cargo_root:
            pending.append((path, DiscoveryContext(postgres=integration_binary_is_postgres(path))))
    while pending:
        path, context = pending.popleft()
        seen = contexts.setdefault(path, set())
        if context in seen:
            continue
        seen.add(context)
        for child, modules in edges.get(path, []):
            pending.append((child, context.under(modules)))
    return contexts


def test_contexts(
    path: Path, modules: tuple[str, ...], index: dict[Path, set[DiscoveryContext]]
) -> set[DiscoveryContext]:
    """Resolve one test's namespace without mixing distinct compilation paths."""
    return {context.under(modules) for context in index.get(path.resolve(), {DiscoveryContext()})}


def package_name(root: Path) -> str:
    """Read Cargo's package name with a Python 3.9-compatible fallback."""
    manifest_path = root / "Cargo.toml"
    if tomllib is not None:
        try:
            with manifest_path.open("rb") as manifest:
                package = tomllib.load(manifest).get("package", {})
        except tomllib.TOMLDecodeError as error:
            raise ValueError(f"invalid Cargo manifest {manifest_path}: {error}") from error
        name = package.get("name")
        if isinstance(name, str) and name:
            return name
    else:
        in_package = False
        for line in manifest_path.read_text(encoding="utf-8").splitlines():
            section = TOML_SECTION.match(line)
            if section is not None:
                in_package = section.group("name").strip() == "package"
                continue
            if in_package and (match := TOML_PACKAGE_NAME.match(line)) is not None:
                return match.group("name")
    raise ValueError(f"discoverable PostgreSQL tests lack a package name: {root}")


def file_has_postgres_lane_test(
    path: Path, out_of_line_modules: dict[Path, set[DiscoveryContext]]
) -> bool:
    source = path.read_text(encoding="utf-8")
    sanitized = sanitize_rust(source)
    ranges = module_ranges(source)

    for attribute_start, _attribute_end, reason in ignore_attributes(source, sanitized):
        reason_lower = reason.lower()
        modules = tuple(name for start, end, name in ranges if start < attribute_start < end)
        if (
            ("postgres" in reason_lower or "postgresql" in reason_lower)
            and not EXTERNAL_INFRA.search(reason)
            and any(context.postgres and not context.external for context in test_contexts(path, modules, out_of_line_modules))
        ):
            return True
    return False


def postgres_packages(
    files: list[Path], out_of_line_modules: dict[Path, set[DiscoveryContext]]
) -> list[str]:
    roots = {
        root
        for path in files
        if file_has_postgres_lane_test(path, out_of_line_modules)
        if (root := crate_root(path)) is not None
    }
    return sorted(package_name(root) for root in roots)


def validate_file(
    path: Path, out_of_line_modules: dict[Path, set[DiscoveryContext]]
) -> list[str]:
    source = path.read_text(encoding="utf-8")
    sanitized = sanitize_rust(source)
    ranges = module_ranges(source)
    errors = []

    for match in BARE_IGNORE_ATTRIBUTE.finditer(sanitized):
        function = FUNCTION.search(sanitized, match.end())
        if function is None:
            errors.append(f"{path}: ignored infrastructure test has no following function")
            continue
        modules = tuple(name for start, end, name in ranges if start < match.start() < end)
        in_postgres_structure = any(context.postgres for context in test_contexts(path, modules, out_of_line_modules))
        if in_postgres_structure:
            errors.append(
                f"{path}:{source.count(chr(10), 0, function.start()) + 1}: "
                f"{function.group('name')} uses bare #[ignore] in PostgreSQL discovery; "
                'use #[ignore = "requires PostgreSQL"] or an explicit external-infra reason'
            )

    for attribute_start, attribute_end, reason in ignore_attributes(source, sanitized):
        reason_lower = reason.lower()
        mentions_postgres = "postgres" in reason_lower or "postgresql" in reason_lower
        mentions_redis = "redis" in reason_lower
        if not mentions_postgres and not mentions_redis:
            continue

        function = FUNCTION.search(sanitized, attribute_end)
        if function is None:
            errors.append(f"{path}: ignored infrastructure test has no following function")
            continue
        function_name = function.group("name")
        modules = tuple(
            name for start, end, name in ranges if start < attribute_start < end
        )
        contexts = test_contexts(path, modules, out_of_line_modules)
        in_postgres_lane = any(context.postgres and not context.external for context in contexts)
        in_external_module = all(context.external for context in contexts)
        needs_external_infra = bool(EXTERNAL_INFRA.search(reason))

        if mentions_redis and not mentions_postgres and in_postgres_lane and not in_external_module:
            errors.append(
                f"{path}:{source.count(chr(10), 0, function.start()) + 1}: "
                f"{function_name} is Redis-only but sits inside PostgreSQL discovery; "
                "move it under an external_infra* module"
            )
        elif needs_external_infra and not in_external_module:
            errors.append(
                f"{path}:{source.count(chr(10), 0, function.start()) + 1}: "
                f"{function_name} requires infrastructure beyond PostgreSQL/Redis; "
                "move it under an external_infra* module"
            )
        elif mentions_postgres and in_external_module and not needs_external_infra:
            errors.append(
                f"{path}:{source.count(chr(10), 0, function.start()) + 1}: "
                f"{function_name} requires only PostgreSQL/Redis but is excluded by an "
                "external_infra* module; move it into PostgreSQL discovery"
            )
        elif mentions_postgres and not needs_external_infra and not in_postgres_lane:
            errors.append(
                f"{path}:{source.count(chr(10), 0, function.start()) + 1}: "
                f"{function_name} requires PostgreSQL but is not discoverable; "
                "place it under postgres_tests or in a postgres_* integration binary"
            )

    return errors


def rust_files(arguments: list[str]) -> list[Path]:
    files = []
    for argument in arguments:
        path = Path(argument)
        if path.is_dir():
            files.extend(candidate for candidate in path.rglob("*.rs") if "target" not in candidate.parts)
        elif path.suffix == ".rs":
            files.append(path)
        else:
            raise ValueError(f"not a Rust source file or directory: {path}")
    return sorted(set(files))


def main() -> int:
    arguments = sys.argv[1:]
    print_packages = bool(arguments and arguments[0] == "--print-packages")
    if print_packages:
        arguments = arguments[1:]
    if not arguments:
        print(
            f"usage: {Path(sys.argv[0]).name} [--print-packages] "
            "<Rust source file or directory> [...]",
            file=sys.stderr,
        )
        return 2
    try:
        files = rust_files(arguments)
    except ValueError as error:
        print(error, file=sys.stderr)
        return 2
    out_of_line_modules = out_of_line_module_index(files)
    errors = [
        error
        for path in files
        for error in validate_file(path, out_of_line_modules)
    ]
    if errors:
        print("PostgreSQL test discovery validation failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    if print_packages:
        try:
            packages = postgres_packages(files, out_of_line_modules)
        except (OSError, ValueError) as error:
            print(error, file=sys.stderr)
            return 1
        for package in packages:
            print(package)
        return 0
    print(f"validated PostgreSQL test discovery across {len(files)} Rust source files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

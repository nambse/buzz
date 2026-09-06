"""Bounded lexical extraction of this repository's SQL function definitions.

This reads source, never executes SQL. It preserves every byte inside a function
and handles clauses after its dollar-quoted body. PostgreSQL remains the parser
and catalog authority; unsupported source forms fail rather than disappear.
"""
from dataclasses import dataclass
import re

MAX_SOURCE_BYTES = 4 * 1024 * 1024
MAX_TOKENS = 250000
_DOLLAR = re.compile(r"\$(?:[A-Za-z_][A-Za-z_0-9]*)?\$")
_WORD = re.compile(r"[A-Za-z_][A-Za-z_0-9$]*")


class SourceError(ValueError):
    """The selected bounded source is malformed or outside the supported shape."""


def _tokens(source):
    if not isinstance(source, str) or len(source.encode("utf-8")) > MAX_SOURCE_BYTES:
        raise SourceError("schema_source_bound")
    index, count = 0, 0
    while index < len(source):
        start = index
        if source[index].isspace():
            index += 1
            continue
        if source.startswith("--", index):
            end = source.find("\n", index + 2)
            index = len(source) if end < 0 else end + 1
            continue
        if source.startswith("/*", index):
            depth = 1
            index += 2
            while depth and index < len(source):
                if source.startswith("/*", index):
                    depth += 1
                    if depth > 32:
                        raise SourceError("schema_comment_bound")
                    index += 2
                elif source.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                raise SourceError("schema_comment_unterminated")
            continue
        dollar = _DOLLAR.match(source, index)
        if dollar:
            tag = dollar.group()
            end = source.find(tag, dollar.end())
            if end < 0:
                raise SourceError("schema_dollar_unterminated")
            index = end + len(tag)
            kind = "dollar"
        elif source[index] in "'\"" or (source[index:index + 1] in ("e", "E")
                                          and source[index + 1:index + 2] == "'"):
            escaped = source[index] in "eE"
            if escaped:
                index += 1
            quote = source[index]
            index += 1
            while index < len(source):
                if escaped and source[index] == "\\":
                    index += 2
                elif source[index] == quote:
                    if source[index + 1:index + 2] == quote:
                        index += 2
                    else:
                        index += 1
                        break
                else:
                    index += 1
            else:
                raise SourceError("schema_quote_unterminated")
            if index > len(source):
                raise SourceError("schema_quote_unterminated")
            kind = "quote"
        else:
            word = _WORD.match(source, index)
            if word:
                index = word.end()
                kind = "word"
            else:
                index += 1
                kind = "symbol"
        count += 1
        if count > MAX_TOKENS:
            raise SourceError("schema_token_bound")
        yield kind, source[start:index], start, index


def statements(source):
    """Yield complete top-level SQL statements without leading comments."""
    start = None
    for _, token, begin, end in _tokens(source):
        if start is None:
            start = begin
        if token == ";":
            yield source[start:end]
            start = None
    if start is not None:
        raise SourceError("schema_statement_unterminated")


@dataclass(frozen=True)
class Function:
    """One exact function statement, including its body delimiter and suffix."""
    name: str
    arguments: str
    statement: str
    body: str
    body_start: int
    body_end: int

    def with_body(self, body):
        """Replace only the body when constructing a reviewed closed bootstrap."""
        return self.statement[:self.body_start] + body + self.statement[self.body_end:]


def functions(source):
    """Yield each function in source order; replacements remain separate entries."""
    for statement in statements(source):
        tokens = list(_tokens(statement))
        words = [token[1].upper() for token in tokens]
        if words[:1] != ["CREATE"]:
            continue
        offset = 3 if words[1:3] == ["OR", "REPLACE"] else 1
        if words[offset:offset + 1] != ["FUNCTION"]:
            continue
        if (len(tokens) < offset + 4 or tokens[offset + 1][0] != "word"
                or words[offset + 2] != "("):
            raise SourceError("schema_function_identity_unsupported")
        name = tokens[offset + 1][1]
        opening = offset + 2
        depth, closing = 1, opening + 1
        while closing < len(tokens) and depth:
            token = tokens[closing][1]
            if token == "(":
                depth += 1
            elif token == ")":
                depth -= 1
            if depth:
                closing += 1
        if depth:
            raise SourceError("schema_function_arguments_unterminated")
        bodies = [index for index in range(closing + 1, len(tokens))
                  if tokens[index][0] == "dollar" and words[index - 1] == "AS"]
        if len(bodies) != 1:
            raise SourceError("schema_function_body_unsupported")
        token = tokens[bodies[0]]
        delimiter = _DOLLAR.match(token[1]).group()
        begin, end = token[2] + len(delimiter), token[3] - len(delimiter)
        yield Function(name, statement[tokens[opening][3]:tokens[closing][2]],
                       statement, statement[begin:end], begin, end)

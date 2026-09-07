"""Read-only metadata of one explicitly selected, quiesced recovery80 database."""

import json
import re

from backup_private_database import COLUMN_ROWS_SQL, SCHEMA_SQL
from private_stack_state import require

DATABASES = {"main": ("ortak", "ortak"), "honcho": ("ortak_honcho", "ortak_honcho_private")}
TABLE_COUNTS = r"""
SELECT jsonb_object_agg(name,n) FROM (
 SELECT format('%I.%I',ns.nspname,c.relname) name,
 ((xpath('/table/row/n/text()',query_to_xml(format('SELECT count(*) n FROM %I.%I',ns.nspname,c.relname),false,false,'')))[1]::text)::bigint n
 FROM pg_class c JOIN pg_namespace ns ON ns.oid=c.relnamespace
 WHERE ns.nspname='public' AND c.relkind IN ('r','p')
) counts;
"""
CONTENT = r"""
SELECT jsonb_object_agg(name,digest) FROM (
 SELECT format('%I.%I',n.nspname,c.relname) name,
 (xpath('/table/row/digest/text()',query_to_xml(format(
  'SELECT encode(sha256(convert_to(COALESCE(string_agg(h,'''' ORDER BY h),''''),''UTF8'')),''hex'') digest FROM (SELECT encode(sha256(convert_to(to_jsonb(t)::text,''UTF8'')),''hex'') h FROM %I.%I t) hashes',
   n.nspname,c.relname),false,false,'')))[1]::text digest
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relkind IN ('r','p')
) hashes;
"""
CATALOG = "WITH live_columns AS (" + COLUMN_ROWS_SQL + "), schema_catalog AS (" + SCHEMA_SQL + ") " + """
SELECT jsonb_object_agg(key,encode(sha256(convert_to(value::text,'UTF8')),'hex'))
FROM schema_catalog,jsonb_each(document);
"""
SEQUENCES = r"""
SELECT COALESCE(jsonb_object_agg(name,body),'{}') FROM (
 SELECT format('%I.%I',n.nspname,c.relname) name,
 ((xpath('/table/row/body/text()',query_to_xml(format(
  'SELECT jsonb_build_object(''last_value'',last_value,''is_called'',is_called) body FROM %I.%I',
   n.nspname,c.relname),false,false,'')))[1]::text)::jsonb body
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='public' AND c.relkind='S'
) sequences;
"""


def metadata(commands, identifier, kind, label):
    """Hash all bounded public row bytes, schema and sequences without exposing their content."""
    require(kind in DATABASES and re.fullmatch(r"[0-9a-f]{64}", identifier))
    role, database = DATABASES[kind]
    argv = commands.docker("exec", "-i", identifier, "psql", "-U", role, "-d", database,
                           "-XAtq", "-v", "ON_ERROR_STOP=1")

    def read(name, sql):
        bounded = "BEGIN READ ONLY; SET LOCAL statement_timeout='30s'; SET LOCAL lock_timeout='2s';\n" + sql + "\nROLLBACK;"
        return json.loads(commands.run(label + "-" + name, argv, sql=bounded, ceiling=512 * 1024))

    scopes = read("scopes", "SELECT jsonb_agg(nspname ORDER BY nspname) FROM pg_namespace WHERE nspname NOT LIKE 'pg_%' AND nspname<>'information_schema';")
    require(scopes == ["public"])
    counts = read("counts", TABLE_COUNTS)
    require(counts and len(counts) <= 2048 and sum(counts.values()) <= 200000)
    rows = read("rows", CONTENT)
    require(set(rows) == set(counts) and all(re.fullmatch(r"[0-9a-f]{64}", value) for value in rows.values()))
    result = {"tables": counts, "logical_rows_sha256": rows, "catalog": read("catalog", CATALOG),
              "sequences": read("sequences", SEQUENCES)}
    if kind == "main":
        versions = read("migrations", "SELECT jsonb_agg(jsonb_build_array(version,encode(checksum,'hex'),success) ORDER BY version) FROM _sqlx_migrations;")
        require([v[0] for v in versions] == list(range(1, 81)) and all(v[2] for v in versions))
        result["migrations"] = versions
    return result

// Ejemplo de cuerpo a partir del esquema OpenAPI: el *Try it* arranca con un
// JSON válido que el endpoint puede recibir, en vez de un `{}` vacío.

const MAX_DEPTH = 6;
const REF_PREFIX = "#/components/schemas/";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function refName(schema: Record<string, unknown>): string | null {
  const ref = schema.$ref;
  if (typeof ref !== "string" || !ref.startsWith(REF_PREFIX)) return null;
  return ref.slice(REF_PREFIX.length);
}

function stringSample(schema: Record<string, unknown>): string {
  const format = typeof schema.format === "string" ? schema.format : "";
  switch (format) {
    case "email":
      return "correo@ejemplo.com";
    case "password":
      return "secreto123";
    case "uuid":
      return "00000000-0000-0000-0000-000000000000";
    case "date":
      return "2026-01-01";
    case "date-time":
      return "2026-01-01T00:00:00Z";
    case "uri":
    case "url":
      return "https://ejemplo.com";
    case "hostname":
      return "ejemplo.com";
    case "ipv4":
      return "127.0.0.1";
    default:
      return "texto";
  }
}

function typeOf(schema: Record<string, unknown>): string {
  if (typeof schema.type === "string") return schema.type;
  if (isRecord(schema.properties)) return "object";
  if (schema.items !== undefined) return "array";
  return "";
}

function sampleOf(
  schema: unknown,
  schemas: Record<string, unknown>,
  seen: ReadonlySet<string>,
  depth: number,
): unknown {
  if (depth > MAX_DEPTH || !isRecord(schema)) return null;

  const name = refName(schema);
  if (name !== null) {
    if (seen.has(name)) return null;
    const target = schemas[name];
    if (target === undefined) return null;
    return sampleOf(target, schemas, new Set(seen).add(name), depth + 1);
  }

  if (schema.const !== undefined) return schema.const;
  if (Array.isArray(schema.enum) && schema.enum.length > 0) return schema.enum[0];
  if (schema.example !== undefined) return schema.example;
  if (schema.default !== undefined) return schema.default;

  switch (typeOf(schema)) {
    case "object": {
      const properties = isRecord(schema.properties) ? schema.properties : {};
      const out: Record<string, unknown> = {};
      for (const [key, property] of Object.entries(properties)) {
        out[key] = sampleOf(property, schemas, seen, depth + 1);
      }
      return out;
    }
    case "array":
      return [];
    case "integer":
    case "number":
      return 0;
    case "boolean":
      return false;
    case "string":
      return stringSample(schema);
    default:
      return null;
  }
}

/** Valor de ejemplo para el cuerpo; `null` cuando el esquema no aporta nada. */
export function sampleFromSchema(
  schema: unknown,
  schemas: Record<string, unknown>,
): unknown {
  return sampleOf(schema, schemas, new Set<string>(), 0);
}

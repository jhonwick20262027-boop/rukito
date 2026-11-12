// Tipos del documento OpenAPI y validación estricta del JSON embebido.
// Nada de `any`: el JSON llega como `unknown` y se estrecha con guards.

export interface Info {
  title: string;
  version: string;
  description?: string;
}

export interface Server {
  url: string;
  description?: string;
}

export interface Tag {
  name: string;
  description?: string;
}

export interface Parameter {
  name: string;
  in: "path" | "query" | "header";
  description?: string;
  required: boolean;
  schema: unknown;
  example?: unknown;
}

export interface MediaType {
  schema?: unknown;
  example?: unknown;
}

export interface RequestBody {
  description?: string;
  required: boolean;
  content: Record<string, MediaType>;
}

export interface ApiResponse {
  description: string;
  content: Record<string, MediaType>;
}

export interface Operation {
  tags: string[];
  operationId?: string;
  summary?: string;
  description?: string;
  parameters: Parameter[];
  requestBody?: RequestBody;
  responses: Record<string, ApiResponse>;
  security: Record<string, string[]>[];
  deprecated: boolean;
}

export interface Components {
  schemas: Record<string, unknown>;
  securitySchemes: Record<string, unknown>;
}

export interface OpenApiDocument {
  openapi: string;
  info: Info;
  servers: Server[];
  tags: Tag[];
  paths: Record<string, Record<string, Operation>>;
  components: Components;
}

export interface OperationEntry {
  method: string;
  path: string;
  operation: Operation;
}

const HTTP_METHODS = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function optionalText(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function list(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function flag(value: unknown): boolean {
  return value === true;
}

function parseMedia(value: unknown): MediaType {
  if (!isRecord(value)) return {};
  return { schema: value.schema, example: value.example };
}

function parseContent(value: unknown): Record<string, MediaType> {
  const content: Record<string, MediaType> = {};
  if (!isRecord(value)) return content;
  for (const [mime, media] of Object.entries(value)) content[mime] = parseMedia(media);
  return content;
}

function parseParameter(value: unknown): Parameter {
  if (!isRecord(value)) {
    return { name: "", in: "query", required: false, schema: {} };
  }
  const location = text(value.in);
  const where = location === "path" || location === "header" ? location : "query";
  return {
    name: text(value.name),
    in: where,
    description: optionalText(value.description),
    required: flag(value.required) || where === "path",
    schema: value.schema,
    example: value.example,
  };
}

function parseResponses(value: unknown): Record<string, ApiResponse> {
  const responses: Record<string, ApiResponse> = {};
  if (!isRecord(value)) return responses;
  for (const [status, response] of Object.entries(value)) {
    if (!isRecord(response)) continue;
    responses[status] = {
      description: text(response.description),
      content: parseContent(response.content),
    };
  }
  return responses;
}

function parseSecurity(value: unknown): Record<string, string[]>[] {
  const requirements: Record<string, string[]>[] = [];
  for (const item of list(value)) {
    if (!isRecord(item)) continue;
    const requirement: Record<string, string[]> = {};
    for (const [scheme, scopes] of Object.entries(item)) {
      requirement[scheme] = list(scopes).filter((scope): scope is string => typeof scope === "string");
    }
    requirements.push(requirement);
  }
  return requirements;
}

function parseOperation(value: unknown): Operation {
  if (!isRecord(value)) {
    return { tags: [], parameters: [], responses: {}, security: [], deprecated: false };
  }
  const body = isRecord(value.requestBody) ? value.requestBody : undefined;
  const requestBody: RequestBody | undefined = body
    ? {
        description: optionalText(body.description),
        required: flag(body.required),
        content: parseContent(body.content),
      }
    : undefined;
  return {
    tags: list(value.tags).filter((tag): tag is string => typeof tag === "string"),
    operationId: optionalText(value.operationId),
    summary: optionalText(value.summary),
    description: optionalText(value.description),
    parameters: list(value.parameters).map(parseParameter),
    requestBody,
    responses: parseResponses(value.responses),
    security: parseSecurity(value.security),
    deprecated: flag(value.deprecated),
  };
}

function parseComponents(value: unknown): Components {
  if (!isRecord(value)) return { schemas: {}, securitySchemes: {} };
  const schemas = isRecord(value.schemas) ? value.schemas : {};
  const securitySchemes = isRecord(value.securitySchemes) ? value.securitySchemes : {};
  return { schemas, securitySchemes };
}

/** Convierte el JSON embebido en un documento; falla con un mensaje legible. */
export function parseDocument(value: unknown): OpenApiDocument {
  if (!isRecord(value)) throw new Error("el documento no es un objeto JSON");
  const info = isRecord(value.info) ? value.info : {};
  const servers: Server[] = list(value.servers)
    .filter(isRecord)
    .map((server) => ({ url: text(server.url), description: optionalText(server.description) }));
  const tags: Tag[] = list(value.tags)
    .filter(isRecord)
    .map((tag) => ({ name: text(tag.name), description: optionalText(tag.description) }));

  const paths: Record<string, Record<string, Operation>> = {};
  const pathsRaw = isRecord(value.paths) ? value.paths : {};
  for (const [path, item] of Object.entries(pathsRaw)) {
    if (!isRecord(item)) continue;
    const operations: Record<string, Operation> = {};
    for (const [method, operation] of Object.entries(item)) {
      if (!HTTP_METHODS.includes(method)) continue;
      operations[method] = parseOperation(operation);
    }
    if (Object.keys(operations).length > 0) paths[path] = operations;
  }

  return {
    openapi: text(value.openapi),
    info: {
      title: text(info.title, "API"),
      version: text(info.version),
      description: optionalText(info.description),
    },
    servers,
    tags,
    paths,
    components: parseComponents(value.components),
  };
}

/** Todas las operaciones del documento, en el orden en que llegaron. */
export function operationEntries(api: OpenApiDocument): OperationEntry[] {
  const entries: OperationEntry[] = [];
  for (const [path, item] of Object.entries(api.paths)) {
    for (const [method, operation] of Object.entries(item)) {
      entries.push({ method, path, operation });
    }
  }
  return entries;
}

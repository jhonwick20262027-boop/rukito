export interface TypeRef {
  kind: string;
  name: string | null;
  ofType: TypeRef | null;
}

export interface InputValue {
  name: string;
  description: string | null;
  type: TypeRef;
  defaultValue: string | null;
}

export interface FieldDef {
  name: string;
  description: string | null;
  args: InputValue[];
  type: TypeRef;
}

export interface EnumValue {
  name: string;
  description: string | null;
}

export interface NamedType {
  kind: string;
  name: string;
  description: string | null;
  fields: FieldDef[] | null;
  inputFields: InputValue[] | null;
  enumValues: EnumValue[] | null;
  possibleTypes: { name: string }[] | null;
  interfaces: { name: string }[] | null;
}

export interface SchemaInfo {
  queryType: string | null;
  mutationType: string | null;
  subscriptionType: string | null;
  types: NamedType[];
}

export type OperationKind = "query" | "mutation" | "subscription";

export interface RunResult {
  status: number;
  statusText: string;
  durationMs: number;
  bytes: number;
  body: unknown;
  text: string;
}

export interface StreamEvent {
  id: number;
  event: string;
  data: unknown;
  raw: string;
  receivedAt: string;
}

export interface HistoryEntry {
  id: string;
  query: string;
  variables: string;
  headers: string;
  kind: string;
  status: number | null;
  at: string;
  durationMs: number;
}

export type ParseResult = { ok: true; value: unknown } | { ok: false; error: string };

export type HeadersResult =
  | { ok: true; value: Record<string, string> }
  | { ok: false; error: string };

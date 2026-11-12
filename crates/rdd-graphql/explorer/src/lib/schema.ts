import type { Completion, CompletionContext, CompletionResult, CompletionSource } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import { hoverTooltip } from "@codemirror/view";

import type { FieldDef, InputValue, NamedType, OperationKind, SchemaInfo, TypeRef } from "./types";
import { arrayOf, isRecord, stringOrNull } from "./utils";

export const INTROSPECTION_QUERY = `query ExplorerSchema {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types {
      kind
      name
      description
      fields {
        name
        description
        args {
          name
          description
          defaultValue
          type { ...TypeRef }
        }
        type { ...TypeRef }
      }
      inputFields {
        name
        description
        defaultValue
        type { ...TypeRef }
      }
      enumValues { name description }
      possibleTypes { name }
      interfaces { name }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType { kind name }
      }
    }
  }
}`;

function parseTypeRef(value: unknown): TypeRef | null {
  if (!isRecord(value)) return null;
  const kind = stringOrNull(value.kind);
  if (kind === null) return null;
  const name = stringOrNull(value.name);
  const inner = value.ofType === null || value.ofType === undefined ? null : parseTypeRef(value.ofType);
  return { kind, name, ofType: inner };
}

function parseInputValue(value: unknown): InputValue | null {
  if (!isRecord(value)) return null;
  const name = stringOrNull(value.name);
  const type = parseTypeRef(value.type);
  if (name === null || type === null) return null;
  return {
    name,
    description: stringOrNull(value.description),
    type,
    defaultValue: stringOrNull(value.defaultValue),
  };
}

function parseField(value: unknown): FieldDef | null {
  if (!isRecord(value)) return null;
  const name = stringOrNull(value.name);
  const type = parseTypeRef(value.type);
  if (name === null || type === null) return null;
  return {
    name,
    description: stringOrNull(value.description),
    args: arrayOf(value.args)
      .map(parseInputValue)
      .filter((item): item is InputValue => item !== null),
    type,
  };
}

function parseNamedType(value: unknown): NamedType | null {
  if (!isRecord(value)) return null;
  const kind = stringOrNull(value.kind);
  const name = stringOrNull(value.name);
  if (kind === null || name === null) return null;

  const fields = Array.isArray(value.fields)
    ? value.fields.map(parseField).filter((item): item is FieldDef => item !== null)
    : null;
  const inputFields = Array.isArray(value.inputFields)
    ? value.inputFields.map(parseInputValue).filter((item): item is InputValue => item !== null)
    : null;
  const enumValues = Array.isArray(value.enumValues)
    ? value.enumValues
        .map((item) => {
          if (!isRecord(item)) return null;
          const enumName = stringOrNull(item.name);
          if (enumName === null) return null;
          return { name: enumName, description: stringOrNull(item.description) };
        })
        .filter((item): item is { name: string; description: string | null } => item !== null)
    : null;
  const nameList = (input: unknown): { name: string }[] | null =>
    Array.isArray(input)
      ? input
          .map((item) => {
            if (!isRecord(item)) return null;
            const itemName = stringOrNull(item.name);
            return itemName === null ? null : { name: itemName };
          })
          .filter((item): item is { name: string } => item !== null)
      : null;

  return {
    kind,
    name,
    description: stringOrNull(value.description),
    fields,
    inputFields,
    enumValues,
    possibleTypes: nameList(value.possibleTypes),
    interfaces: nameList(value.interfaces),
  };
}

function rootName(value: unknown): string | null {
  if (!isRecord(value)) return null;
  return stringOrNull(value.name);
}

export function buildSchemaInfo(body: unknown): SchemaInfo | null {
  if (!isRecord(body)) return null;
  const data = body.data;
  if (!isRecord(data)) return null;
  const schema = data.__schema;
  if (!isRecord(schema)) return null;
  const types = arrayOf(schema.types)
    .map(parseNamedType)
    .filter((item): item is NamedType => item !== null);
  if (types.length === 0) return null;
  return {
    queryType: rootName(schema.queryType),
    mutationType: rootName(schema.mutationType),
    subscriptionType: rootName(schema.subscriptionType),
    types,
  };
}

export function formatType(reference: TypeRef): string {
  if (reference.kind === "NON_NULL" && reference.ofType !== null) {
    return `${formatType(reference.ofType)}!`;
  }
  if (reference.kind === "LIST" && reference.ofType !== null) {
    return `[${formatType(reference.ofType)}]`;
  }
  return reference.name ?? "?";
}

export function namedTypeName(reference: TypeRef): string | null {
  let current: TypeRef | null = reference;
  while (current !== null && current.name === null) {
    current = current.ofType;
  }
  return current?.name ?? null;
}

const KIND_LABELS: Record<string, string> = {
  OBJECT: "Objeto",
  INPUT_OBJECT: "Entrada",
  SCALAR: "Escalar",
  ENUM: "Enum",
  INTERFACE: "Interfaz",
  UNION: "Unión",
};

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

export function schemaTypes(schema: SchemaInfo, includeIntrospection: boolean): NamedType[] {
  return schema.types
    .filter((type) => includeIntrospection || !type.name.startsWith("__"))
    .slice()
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function rootTypeName(schema: SchemaInfo, kind: OperationKind): string | null {
  if (kind === "mutation") return schema.mutationType;
  if (kind === "subscription") return schema.subscriptionType;
  return schema.queryType;
}

interface DocToken {
  name: boolean;
  value: string;
}

function tokenize(input: string, end: number): DocToken[] {
  const tokens: DocToken[] = [];
  let index = 0;
  while (index < end) {
    const char = input.charAt(index);
    if (char === "#") {
      while (index < end && input.charAt(index) !== "\n") index += 1;
      continue;
    }
    if (char === '"') {
      if (input.startsWith('"""', index)) {
        index += 3;
        while (index < end && !input.startsWith('"""', index)) index += 1;
        index = Math.min(end, index + 3);
        continue;
      }
      index += 1;
      let escaped = false;
      while (index < end) {
        const current = input.charAt(index);
        index += 1;
        if (escaped) {
          escaped = false;
          continue;
        }
        if (current === "\\") {
          escaped = true;
          continue;
        }
        if (current === '"') break;
      }
      continue;
    }
    if (char === "." && input.startsWith("...", index)) {
      tokens.push({ name: false, value: "..." });
      index += 3;
      continue;
    }
    if (/[_A-Za-z]/.test(char)) {
      const start = index;
      while (index < end && /[_0-9A-Za-z]/.test(input.charAt(index))) index += 1;
      tokens.push({ name: true, value: input.slice(start, index) });
      continue;
    }
    if ("{}()[]:".includes(char)) {
      tokens.push({ name: false, value: char });
      index += 1;
      continue;
    }
    index += 1;
  }
  return tokens;
}

function lexicalAt(input: string, end: number): "comment" | "string" | null {
  let mode: "normal" | "comment" | "string" | "block" = "normal";
  let index = 0;
  while (index < end) {
    const char = input.charAt(index);
    if (mode === "normal") {
      if (char === "#") {
        mode = "comment";
        index += 1;
        continue;
      }
      if (char === '"') {
        if (input.startsWith('"""', index)) {
          mode = "block";
          index += 3;
          continue;
        }
        mode = "string";
        index += 1;
        continue;
      }
      index += 1;
      continue;
    }
    if (mode === "comment") {
      if (char === "\n") mode = "normal";
      index += 1;
      continue;
    }
    if (mode === "string") {
      if (char === "\\") {
        index += 2;
        continue;
      }
      if (char === '"') mode = "normal";
      index += 1;
      continue;
    }
    if (char === '"' && input.startsWith('"""', index)) {
      mode = "normal";
      index += 3;
      continue;
    }
    index += 1;
  }
  if (mode === "comment") return "comment";
  if (mode === "string" || mode === "block") return "string";
  return null;
}

interface Frame {
  type: NamedType | null;
  field: FieldDef | null;
}

interface Scope {
  type: NamedType | null;
  field: FieldDef | null;
  depth: number;
  parenDepth: number;
}

function typesByName(schema: SchemaInfo): Map<string, NamedType> {
  const map = new Map<string, NamedType>();
  for (const type of schema.types) map.set(type.name, type);
  return map;
}

function scopeAt(input: string, end: number, schema: SchemaInfo): Scope {
  const tokens = tokenize(input, end);
  const byName = typesByName(schema);
  const frames: Frame[] = [];
  const names: string[] = [];
  const parens: number[] = [];
  let parenDepth = 0;
  let kind: OperationKind = "query";

  const rootType = (): NamedType | null => {
    const name = rootTypeName(schema, kind);
    return name === null ? null : byName.get(name) ?? null;
  };

  for (const token of tokens) {
    if (token.name) {
      if (token.value === "query" || token.value === "mutation" || token.value === "subscription") {
        if (frames.length === 0 && parenDepth === 0 && names.length === 0) {
          kind = token.value;
          continue;
        }
      }
      if (token.value === "on") {
        names.push(token.value);
        continue;
      }
      names.push(token.value);
      continue;
    }
    if (token.value === "(") {
      parens.push(names.length);
      parenDepth += 1;
      continue;
    }
    if (token.value === ")") {
      const restore = parens.pop();
      if (restore !== undefined) names.length = restore;
      parenDepth = Math.max(0, parenDepth - 1);
      continue;
    }
    if (token.value === "{" && parenDepth === 0) {
      const parent = frames.length > 0 ? frames[frames.length - 1] : undefined;
      const parentType = parent?.type ?? rootType();
      const last = names.length > 0 ? names[names.length - 1] ?? null : null;
      const before = names.length > 1 ? names[names.length - 2] ?? null : null;
      let nextType: NamedType | null = null;
      let nextField: FieldDef | null = null;

      if (before === "on" && last !== null) {
        nextType = byName.get(last) ?? null;
      } else if (last !== null && parentType !== null) {
        const field = parentType.fields?.find((item) => item.name === last) ?? null;
        if (field !== null) {
          nextField = field;
          const typeName = namedTypeName(field.type);
          nextType = typeName === null ? null : byName.get(typeName) ?? null;
        }
      }
      if (nextType === null && frames.length === 0) nextType = rootType();

      frames.push({ type: nextType, field: nextField });
      names.length = 0;
      continue;
    }
    if (token.value === "}" && parenDepth === 0) {
      frames.pop();
      names.length = 0;
    }
  }

  const top = frames.length > 0 ? frames[frames.length - 1] : undefined;
  return {
    type: top?.type ?? rootType(),
    field: top?.field ?? null,
    depth: frames.length,
    parenDepth,
  };
}

function typeSuggestion(type: NamedType): Completion {
  return {
    label: type.name,
    type: "type",
    detail: kindLabel(type.kind),
    info: type.description ?? undefined,
  };
}

export function makeCompletionSource(getSchema: () => SchemaInfo | null): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const schema = getSchema();
    if (schema === null) return null;
    const doc = context.state.doc.toString();
    if (lexicalAt(doc, context.pos) !== null) return null;
    const word = context.matchBefore(/[_A-Za-z][_0-9A-Za-z]*/);
    if (word === null && !context.explicit) return null;

    const scope = scopeAt(doc, context.pos, schema);
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const expectsType = /(?:^|\s)on\s+$/.test(before);
    const options: Completion[] = [];

    if (expectsType) {
      for (const type of schemaTypes(schema, false)) {
        if (type.kind === "OBJECT" || type.kind === "INTERFACE" || type.kind === "UNION") {
          options.push(typeSuggestion(type));
        }
      }
    } else if (scope.parenDepth > 0 && scope.field !== null) {
      for (const arg of scope.field.args) {
        options.push({
          label: arg.name,
          type: "property",
          detail: formatType(arg.type),
          info: arg.description ?? undefined,
          apply: `${arg.name}: `,
        });
      }
    } else if (scope.depth > 0 && scope.type !== null) {
      for (const field of scope.type.fields ?? []) {
        options.push({
          label: field.name,
          type: "property",
          detail: formatType(field.type),
          info: field.description ?? undefined,
        });
      }
      options.push({
        label: "__typename",
        type: "property",
        detail: "String!",
        info: "Nombre del tipo en tiempo de ejecución",
      });
    } else {
      for (const keyword of ["query", "mutation", "subscription", "fragment"]) {
        options.push({ label: keyword, type: "keyword" });
      }
    }

    if (options.length === 0) return null;
    return {
      from: word === null ? context.pos : word.from,
      options,
      validFor: /^[_0-9A-Za-z]*$/,
    };
  };
}

function docTooltip(name: string, detail: string, description: string | null): { dom: HTMLElement } {
  const dom = document.createElement("div");
  dom.className = "cm-rdd-doc";
  const head = document.createElement("div");
  const nameNode = document.createElement("span");
  nameNode.className = "cm-rdd-doc-name";
  nameNode.textContent = name;
  head.append(nameNode);
  const typeNode = document.createElement("span");
  typeNode.className = "cm-rdd-doc-type";
  typeNode.textContent = detail;
  head.append(typeNode);
  dom.append(head);
  if (description !== null && description !== "") {
    const text = document.createElement("div");
    text.className = "cm-rdd-doc-text";
    text.textContent = description;
    dom.append(text);
  }
  return { dom };
}

export function schemaHover(getSchema: () => SchemaInfo | null): Extension {
  return hoverTooltip((view, pos) => {
    const schema = getSchema();
    if (schema === null) return null;
    const doc = view.state.doc.toString();
    if (lexicalAt(doc, pos) !== null) return null;
    const word = view.state.wordAt(pos);
    if (word === null) return null;
    const name = view.state.sliceDoc(word.from, word.to);
    if (name === "") return null;

    const scope = scopeAt(doc, word.from, schema);
    const field = scope.type?.fields?.find((item) => item.name === name) ?? null;
    const arg = scope.field?.args.find((item) => item.name === name) ?? null;
    const input = scope.type?.inputFields?.find((item) => item.name === name) ?? null;
    const ownType = schema.types.find((item) => item.name === name) ?? null;

    let detail: string;
    let description: string | null;
    if (field !== null) {
      detail = formatType(field.type);
      description = field.description;
    } else if (arg !== null) {
      detail = formatType(arg.type);
      description = arg.description;
    } else if (input !== null) {
      detail = formatType(input.type);
      description = input.description;
    } else if (ownType !== null) {
      detail = kindLabel(ownType.kind);
      description = ownType.description;
    } else {
      return null;
    }

    return {
      pos: word.from,
      end: word.to,
      above: true,
      create: () => docTooltip(name, detail, description),
    };
  });
}

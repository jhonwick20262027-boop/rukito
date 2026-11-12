import { StreamLanguage, type StreamParser } from "@codemirror/language";

interface GraphqlState {
  block: boolean;
}

const KEYWORDS = new Set([
  "query",
  "mutation",
  "subscription",
  "fragment",
  "on",
  "schema",
  "scalar",
  "type",
  "enum",
  "interface",
  "union",
  "input",
  "implements",
  "directive",
  "extend",
]);

const ATOMS = new Set(["true", "false", "null"]);

const parser: StreamParser<GraphqlState> = {
  name: "graphql",
  startState: () => ({ block: false }),
  token(stream, state) {
    if (state.block) {
      while (!stream.eol()) {
        if (stream.match('"""')) {
          state.block = false;
          return "string";
        }
        stream.next();
      }
      return "string";
    }

    if (stream.eatSpace()) return null;

    if (stream.match("...")) return "punctuation";

    const first = stream.peek();
    if (first === "#") {
      stream.skipToEnd();
      return "comment";
    }

    if (stream.match('"""')) {
      while (!stream.eol()) {
        if (stream.match('"""')) return "string";
        stream.next();
      }
      state.block = true;
      return "string";
    }

    if (first === '"') {
      stream.next();
      let escaped = false;
      while (!stream.eol()) {
        const char = stream.next();
        if (char === undefined) break;
        if (escaped) {
          escaped = false;
          continue;
        }
        if (char === "\\") {
          escaped = true;
          continue;
        }
        if (char === '"') break;
      }
      return "string";
    }

    if (stream.match(/\$[_A-Za-z][_0-9A-Za-z]*/)) return "variableName";
    if (stream.match(/@[_A-Za-z][_0-9A-Za-z]*/)) return "meta";
    if (stream.match(/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/)) return "number";

    if (stream.match(/[_A-Za-z][_0-9A-Za-z]*/)) {
      const word = stream.current();
      if (KEYWORDS.has(word)) return "keyword";
      if (ATOMS.has(word)) return "atom";
      if (/^[A-Z]/.test(word)) return "typeName";
      return "propertyName";
    }

    const char = stream.next();
    if (char !== undefined && "{}()[]:=!|&,".includes(char)) return "punctuation";
    return null;
  },
};

export const graphqlLanguage = StreamLanguage.define(parser);

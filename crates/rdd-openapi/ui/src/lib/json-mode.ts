import { StreamLanguage, type StreamParser } from "@codemirror/language";

interface JsonState {
  depth: number;
  inString: boolean;
}

const parser: StreamParser<JsonState> = {
  name: "json",
  startState: () => ({ depth: 0, inString: false }),
  copyState: (state) => ({ ...state }),
  token(stream, state) {
    if (state.inString) {
      while (!stream.eol()) {
        const char = stream.next();
        if (char === "\\") {
          stream.next();
          continue;
        }
        if (char === '"') {
          state.inString = false;
          break;
        }
      }
      return "string";
    }

    if (stream.eatSpace()) return null;

    const first = stream.peek();
    if (first === '"') {
      stream.next();
      state.inString = true;
      return "string";
    }
    if (first === "{" || first === "[") {
      stream.next();
      state.depth += 1;
      return "punctuation";
    }
    if (first === "}" || first === "]") {
      stream.next();
      state.depth = Math.max(0, state.depth - 1);
      return "punctuation";
    }
    if (first === ":" || first === ",") {
      stream.next();
      return "punctuation";
    }

    if (stream.match(/-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/)) return "number";
    if (stream.match(/true|false/)) return "bool";
    if (stream.match(/null/)) return "null";

    stream.next();
    return "invalid";
  },
  indent(state, textAfter, context) {
    const closing = /^[}\]]/.test(textAfter.trimStart());
    return Math.max(0, state.depth - (closing ? 1 : 0)) * context.unit;
  },
  languageData: {
    indentOnInput: /^\s*[}\]]/,
  },
};

export const jsonLanguage = StreamLanguage.define(parser);

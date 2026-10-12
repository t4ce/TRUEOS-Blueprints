var parse5 = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __defNormalProp = (obj, key, value) => key in obj ? __defProp(obj, key, { enumerable: true, configurable: true, writable: true, value }) : obj[key] = value;
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);
  var __publicField = (obj, key, value) => __defNormalProp(obj, typeof key !== "symbol" ? key + "" : key, value);

  // src/index.ts
  var index_exports = {};
  __export(index_exports, {
    ErrorCodes: () => ERR,
    Parser: () => Parser,
    Token: () => token_exports,
    Tokenizer: () => Tokenizer,
    TokenizerMode: () => TokenizerMode,
    defaultTreeAdapter: () => defaultTreeAdapter,
    foreignContent: () => foreign_content_exports,
    html: () => html_exports,
    parse: () => parse,
    parseFragment: () => parseFragment,
    serialize: () => serialize,
    serializeOuter: () => serializeOuter
  });

  // src/common/unicode.ts
  var UNDEFINED_CODE_POINTS = /* @__PURE__ */ new Set([
    65534,
    65535,
    131070,
    131071,
    196606,
    196607,
    262142,
    262143,
    327678,
    327679,
    393214,
    393215,
    458750,
    458751,
    524286,
    524287,
    589822,
    589823,
    655358,
    655359,
    720894,
    720895,
    786430,
    786431,
    851966,
    851967,
    917502,
    917503,
    983038,
    983039,
    1048574,
    1048575,
    1114110,
    1114111
  ]);
  var REPLACEMENT_CHARACTER = "\uFFFD";
  var SEQUENCES = {
    DASH_DASH: "--",
    CDATA_START: "[CDATA[",
    DOCTYPE: "doctype",
    SCRIPT: "script",
    PUBLIC: "public",
    SYSTEM: "system"
  };
  function isSurrogate(cp) {
    return cp >= 55296 && cp <= 57343;
  }
  function isSurrogatePair(cp) {
    return cp >= 56320 && cp <= 57343;
  }
  function getSurrogatePairCodePoint(cp1, cp2) {
    return (cp1 - 55296) * 1024 + 9216 + cp2;
  }
  function isControlCodePoint(cp) {
    return cp !== 32 && cp !== 10 && cp !== 13 && cp !== 9 && cp !== 12 && cp >= 1 && cp <= 31 || cp >= 127 && cp <= 159;
  }
  function isUndefinedCodePoint(cp) {
    return cp >= 64976 && cp <= 65007 || UNDEFINED_CODE_POINTS.has(cp);
  }

  // src/common/error-codes.ts
  var ERR = /* @__PURE__ */ ((ERR2) => {
    ERR2["controlCharacterInInputStream"] = "control-character-in-input-stream";
    ERR2["noncharacterInInputStream"] = "noncharacter-in-input-stream";
    ERR2["surrogateInInputStream"] = "surrogate-in-input-stream";
    ERR2["nonVoidHtmlElementStartTagWithTrailingSolidus"] = "non-void-html-element-start-tag-with-trailing-solidus";
    ERR2["endTagWithAttributes"] = "end-tag-with-attributes";
    ERR2["endTagWithTrailingSolidus"] = "end-tag-with-trailing-solidus";
    ERR2["unexpectedSolidusInTag"] = "unexpected-solidus-in-tag";
    ERR2["unexpectedNullCharacter"] = "unexpected-null-character";
    ERR2["unexpectedQuestionMarkInsteadOfTagName"] = "unexpected-question-mark-instead-of-tag-name";
    ERR2["invalidFirstCharacterOfTagName"] = "invalid-first-character-of-tag-name";
    ERR2["unexpectedEqualsSignBeforeAttributeName"] = "unexpected-equals-sign-before-attribute-name";
    ERR2["missingEndTagName"] = "missing-end-tag-name";
    ERR2["unexpectedCharacterInAttributeName"] = "unexpected-character-in-attribute-name";
    ERR2["unknownNamedCharacterReference"] = "unknown-named-character-reference";
    ERR2["missingSemicolonAfterCharacterReference"] = "missing-semicolon-after-character-reference";
    ERR2["unexpectedCharacterAfterDoctypeSystemIdentifier"] = "unexpected-character-after-doctype-system-identifier";
    ERR2["unexpectedCharacterInUnquotedAttributeValue"] = "unexpected-character-in-unquoted-attribute-value";
    ERR2["eofBeforeTagName"] = "eof-before-tag-name";
    ERR2["eofInTag"] = "eof-in-tag";
    ERR2["missingAttributeValue"] = "missing-attribute-value";
    ERR2["missingWhitespaceBetweenAttributes"] = "missing-whitespace-between-attributes";
    ERR2["missingWhitespaceAfterDoctypePublicKeyword"] = "missing-whitespace-after-doctype-public-keyword";
    ERR2["missingWhitespaceBetweenDoctypePublicAndSystemIdentifiers"] = "missing-whitespace-between-doctype-public-and-system-identifiers";
    ERR2["missingWhitespaceAfterDoctypeSystemKeyword"] = "missing-whitespace-after-doctype-system-keyword";
    ERR2["missingQuoteBeforeDoctypePublicIdentifier"] = "missing-quote-before-doctype-public-identifier";
    ERR2["missingQuoteBeforeDoctypeSystemIdentifier"] = "missing-quote-before-doctype-system-identifier";
    ERR2["missingDoctypePublicIdentifier"] = "missing-doctype-public-identifier";
    ERR2["missingDoctypeSystemIdentifier"] = "missing-doctype-system-identifier";
    ERR2["abruptDoctypePublicIdentifier"] = "abrupt-doctype-public-identifier";
    ERR2["abruptDoctypeSystemIdentifier"] = "abrupt-doctype-system-identifier";
    ERR2["cdataInHtmlContent"] = "cdata-in-html-content";
    ERR2["incorrectlyOpenedComment"] = "incorrectly-opened-comment";
    ERR2["eofInScriptHtmlCommentLikeText"] = "eof-in-script-html-comment-like-text";
    ERR2["eofInDoctype"] = "eof-in-doctype";
    ERR2["nestedComment"] = "nested-comment";
    ERR2["abruptClosingOfEmptyComment"] = "abrupt-closing-of-empty-comment";
    ERR2["eofInComment"] = "eof-in-comment";
    ERR2["incorrectlyClosedComment"] = "incorrectly-closed-comment";
    ERR2["eofInCdata"] = "eof-in-cdata";
    ERR2["absenceOfDigitsInNumericCharacterReference"] = "absence-of-digits-in-numeric-character-reference";
    ERR2["nullCharacterReference"] = "null-character-reference";
    ERR2["surrogateCharacterReference"] = "surrogate-character-reference";
    ERR2["characterReferenceOutsideUnicodeRange"] = "character-reference-outside-unicode-range";
    ERR2["controlCharacterReference"] = "control-character-reference";
    ERR2["noncharacterCharacterReference"] = "noncharacter-character-reference";
    ERR2["missingWhitespaceBeforeDoctypeName"] = "missing-whitespace-before-doctype-name";
    ERR2["missingDoctypeName"] = "missing-doctype-name";
    ERR2["invalidCharacterSequenceAfterDoctypeName"] = "invalid-character-sequence-after-doctype-name";
    ERR2["duplicateAttribute"] = "duplicate-attribute";
    ERR2["nonConformingDoctype"] = "non-conforming-doctype";
    ERR2["missingDoctype"] = "missing-doctype";
    ERR2["misplacedDoctype"] = "misplaced-doctype";
    ERR2["endTagWithoutMatchingOpenElement"] = "end-tag-without-matching-open-element";
    ERR2["closingOfElementWithOpenChildElements"] = "closing-of-element-with-open-child-elements";
    ERR2["disallowedContentInNoscriptInHead"] = "disallowed-content-in-noscript-in-head";
    ERR2["openElementsLeftAfterEof"] = "open-elements-left-after-eof";
    ERR2["abandonedHeadElementChild"] = "abandoned-head-element-child";
    ERR2["misplacedStartTagForHeadElement"] = "misplaced-start-tag-for-head-element";
    ERR2["nestedNoscriptInHead"] = "nested-noscript-in-head";
    ERR2["eofInElementThatCanContainOnlyText"] = "eof-in-element-that-can-contain-only-text";
    return ERR2;
  })(ERR || {});

  // src/tokenizer/preprocessor.ts
  var DEFAULT_BUFFER_WATERLINE = 1 << 16;
  var Preprocessor = class {
    constructor(handler) {
      this.handler = handler;
      __publicField(this, "html", "");
      __publicField(this, "pos", -1);
      // NOTE: Initial `lastGapPos` is -2, to ensure `col` on initialisation is 0
      __publicField(this, "lastGapPos", -2);
      __publicField(this, "gapStack", []);
      __publicField(this, "skipNextNewLine", false);
      __publicField(this, "lastChunkWritten", false);
      __publicField(this, "endOfChunkHit", false);
      __publicField(this, "bufferWaterline", DEFAULT_BUFFER_WATERLINE);
      __publicField(this, "isEol", false);
      __publicField(this, "lineStartPos", 0);
      __publicField(this, "droppedBufferSize", 0);
      __publicField(this, "line", 1);
      //NOTE: avoid reporting errors twice on advance/retreat
      __publicField(this, "lastErrOffset", -1);
    }
    /** The column on the current line. If we just saw a gap (eg. a surrogate pair), return the index before. */
    get col() {
      return this.pos - this.lineStartPos + Number(this.lastGapPos !== this.pos);
    }
    get offset() {
      return this.droppedBufferSize + this.pos;
    }
    getError(code, cpOffset) {
      const { line, col, offset } = this;
      const startCol = col + cpOffset;
      const startOffset = offset + cpOffset;
      return {
        code,
        startLine: line,
        endLine: line,
        startCol,
        endCol: startCol,
        startOffset,
        endOffset: startOffset
      };
    }
    _err(code) {
      if (this.handler.onParseError && this.lastErrOffset !== this.offset) {
        this.lastErrOffset = this.offset;
        this.handler.onParseError(this.getError(code, 0));
      }
    }
    _addGap() {
      this.gapStack.push(this.lastGapPos);
      this.lastGapPos = this.pos;
    }
    _processSurrogate(cp) {
      if (this.pos !== this.html.length - 1) {
        const nextCp = this.html.charCodeAt(this.pos + 1);
        if (isSurrogatePair(nextCp)) {
          this.pos++;
          this._addGap();
          return getSurrogatePairCodePoint(cp, nextCp);
        }
      } else if (!this.lastChunkWritten) {
        this.endOfChunkHit = true;
        return -1 /* EOF */;
      }
      this._err("surrogate-in-input-stream" /* surrogateInInputStream */);
      return cp;
    }
    willDropParsedChunk() {
      return this.pos > this.bufferWaterline;
    }
    dropParsedChunk() {
      if (this.willDropParsedChunk()) {
        this.html = this.html.substring(this.pos);
        this.lineStartPos -= this.pos;
        this.droppedBufferSize += this.pos;
        this.pos = 0;
        this.lastGapPos = -2;
        this.gapStack.length = 0;
      }
    }
    write(chunk, isLastChunk) {
      if (this.html.length > 0) {
        this.html += chunk;
      } else {
        this.html = chunk;
      }
      this.endOfChunkHit = false;
      this.lastChunkWritten = isLastChunk;
    }
    insertHtmlAtCurrentPos(chunk) {
      this.html = this.html.substring(0, this.pos + 1) + chunk + this.html.substring(this.pos + 1);
      this.endOfChunkHit = false;
    }
    startsWith(pattern, caseSensitive) {
      if (this.pos + pattern.length > this.html.length) {
        this.endOfChunkHit = !this.lastChunkWritten;
        return false;
      }
      if (caseSensitive) {
        return this.html.startsWith(pattern, this.pos);
      }
      for (let i = 0; i < pattern.length; i++) {
        const cp = this.html.charCodeAt(this.pos + i) | 32;
        if (cp !== pattern.charCodeAt(i)) {
          return false;
        }
      }
      return true;
    }
    peek(offset) {
      const pos = this.pos + offset;
      if (pos >= this.html.length) {
        this.endOfChunkHit = !this.lastChunkWritten;
        return -1 /* EOF */;
      }
      const code = this.html.charCodeAt(pos);
      return code === 13 /* CARRIAGE_RETURN */ ? 10 /* LINE_FEED */ : code;
    }
    advance() {
      this.pos++;
      if (this.isEol) {
        this.isEol = false;
        this.line++;
        this.lineStartPos = this.pos;
      }
      if (this.pos >= this.html.length) {
        this.endOfChunkHit = !this.lastChunkWritten;
        return -1 /* EOF */;
      }
      let cp = this.html.charCodeAt(this.pos);
      if (cp === 13 /* CARRIAGE_RETURN */) {
        this.isEol = true;
        this.skipNextNewLine = true;
        return 10 /* LINE_FEED */;
      }
      if (cp === 10 /* LINE_FEED */) {
        this.isEol = true;
        if (this.skipNextNewLine) {
          this.line--;
          this.skipNextNewLine = false;
          this._addGap();
          return this.advance();
        }
      }
      this.skipNextNewLine = false;
      if (isSurrogate(cp)) {
        cp = this._processSurrogate(cp);
      }
      const isCommonValidRange = this.handler.onParseError === null || cp > 31 && cp < 127 || cp === 10 /* LINE_FEED */ || cp === 13 /* CARRIAGE_RETURN */ || cp > 159 && cp < 64976;
      if (!isCommonValidRange) {
        this._checkForProblematicCharacters(cp);
      }
      return cp;
    }
    _checkForProblematicCharacters(cp) {
      if (isControlCodePoint(cp)) {
        this._err("control-character-in-input-stream" /* controlCharacterInInputStream */);
      } else if (isUndefinedCodePoint(cp)) {
        this._err("noncharacter-in-input-stream" /* noncharacterInInputStream */);
      }
    }
    retreat(count) {
      this.pos -= count;
      while (this.pos < this.lastGapPos) {
        this.lastGapPos = this.gapStack.pop();
        this.pos--;
      }
      this.isEol = false;
    }
  };

  // src/common/token.ts
  var token_exports = {};
  __export(token_exports, {
    TokenType: () => TokenType,
    getTokenAttr: () => getTokenAttr
  });
  var TokenType = /* @__PURE__ */ ((TokenType2) => {
    TokenType2[TokenType2["CHARACTER"] = 0] = "CHARACTER";
    TokenType2[TokenType2["NULL_CHARACTER"] = 1] = "NULL_CHARACTER";
    TokenType2[TokenType2["WHITESPACE_CHARACTER"] = 2] = "WHITESPACE_CHARACTER";
    TokenType2[TokenType2["START_TAG"] = 3] = "START_TAG";
    TokenType2[TokenType2["END_TAG"] = 4] = "END_TAG";
    TokenType2[TokenType2["COMMENT"] = 5] = "COMMENT";
    TokenType2[TokenType2["DOCTYPE"] = 6] = "DOCTYPE";
    TokenType2[TokenType2["EOF"] = 7] = "EOF";
    TokenType2[TokenType2["HIBERNATION"] = 8] = "HIBERNATION";
    return TokenType2;
  })(TokenType || {});
  function getTokenAttr(token, attrName) {
    for (let i = token.attrs.length - 1; i >= 0; i--) {
      if (token.attrs[i].name === attrName) {
        return token.attrs[i].value;
      }
    }
    return null;
  }

  // node_modules/entities/dist/decode-codepoint.js
  var c1 = [
    8364,
    0,
    8218,
    402,
    8222,
    8230,
    8224,
    8225,
    710,
    8240,
    352,
    8249,
    338,
    0,
    381,
    0,
    0,
    8216,
    8217,
    8220,
    8221,
    8226,
    8211,
    8212,
    732,
    8482,
    353,
    8250,
    339,
    0,
    382,
    376
  ];
  function isInvalidCodePoint(codePoint) {
    return codePoint === 0 || codePoint >= 55296 && codePoint <= 57343 || codePoint > 1114111;
  }
  function replaceCodePoint(codePoint) {
    if (isInvalidCodePoint(codePoint)) {
      return 65533;
    }
    if (codePoint >= 128 && codePoint <= 159) {
      return c1[codePoint - 128] || codePoint;
    }
    return codePoint;
  }
  function replaceCodePointXML(codePoint) {
    return isInvalidCodePoint(codePoint) ? 65533 : codePoint;
  }

  // node_modules/entities/dist/internal/decode-shared.js
  var BASE91_INVERSE = /* @__PURE__ */ (() => {
    const table = new Uint8Array(127);
    let code = 0;
    for (let char = 33; char <= 126; char++) {
      if (char !== 34 && char !== 36 && char !== 92) {
        table[char] = code++;
      }
    }
    return table;
  })();
  function decodeTrieDict(input, resultLength, atomCount, dict1AtomCount, ngramCount, dictSize) {
    const base = 91;
    const inputLength = input.length;
    const twoCharBias = dictSize * (base - 1);
    let pos = 0;
    const readSlotCode = () => {
      const c12 = BASE91_INVERSE[input.charCodeAt(pos++)];
      return c12 < dictSize ? c12 : c12 * base - twoCharBias + BASE91_INVERSE[input.charCodeAt(pos++)];
    };
    const dict2AtomCount = atomCount - dict1AtomCount;
    const slotCount = atomCount + ngramCount;
    const single = new Int32Array(slotCount);
    single.fill(-1, dict1AtomCount, dictSize);
    single.fill(-1, dictSize + dict2AtomCount, slotCount);
    const start = new Int32Array(slotCount);
    const length = new Int32Array(slotCount);
    function decodeDelta(count, off) {
      let previous = 0;
      let slot = off;
      const end = off + count;
      while (slot < end) {
        const code = BASE91_INVERSE[input.charCodeAt(pos++)];
        if (code < 89) {
          previous += code;
          single[slot++] = previous;
        } else if (code === 89) {
          let runLength = BASE91_INVERSE[input.charCodeAt(pos++)] + 2;
          while (runLength--)
            single[slot++] = ++previous;
        } else {
          const next = BASE91_INVERSE[input.charCodeAt(pos++)];
          previous += 89 + // eslint-disable-next-line unicorn/prefer-minimal-ternary -- branches read a different number of side-effecting input bytes
          (next < 90 ? next * base + BASE91_INVERSE[input.charCodeAt(pos++)] : BASE91_INVERSE[input.charCodeAt(pos++)] * 8281 + BASE91_INVERSE[input.charCodeAt(pos++)] * base + BASE91_INVERSE[input.charCodeAt(pos++)]);
          single[slot++] = previous;
        }
      }
    }
    decodeDelta(dict1AtomCount, 0);
    decodeDelta(dict2AtomCount, dictSize);
    const references = new Int32Array(ngramCount * 2);
    let poolSize = 0;
    let ngramIndex = 0;
    function readNgramReferences(count, startSlot) {
      for (let index = 0; index < count; index++) {
        const slot = startSlot + index;
        const a = readSlotCode();
        const b = readSlotCode();
        references[ngramIndex * 2] = a;
        references[ngramIndex * 2 + 1] = b;
        ngramIndex += 1;
        start[slot] = poolSize;
        const entryLength = (single[a] < 0 ? length[a] : 1) + (single[b] < 0 ? length[b] : 1);
        length[slot] = entryLength;
        poolSize += entryLength;
      }
    }
    readNgramReferences(ngramCount - dictSize + dict1AtomCount, dictSize + dict2AtomCount);
    readNgramReferences(dictSize - dict1AtomCount, dict1AtomCount);
    const pool = new Uint16Array(poolSize);
    let write = 0;
    for (let index = 0; index < ngramIndex; index++) {
      for (let half = 0; half < 2; half++) {
        const source = references[index * 2 + half];
        const value = single[source];
        if (value < 0) {
          let read = start[source];
          const readEnd = read + length[source];
          while (read < readEnd)
            pool[write++] = pool[read++];
        } else {
          pool[write++] = value;
        }
      }
    }
    const out = new Uint16Array(resultLength);
    let outIndex = 0;
    while (pos < inputLength) {
      let slot = BASE91_INVERSE[input.charCodeAt(pos++)];
      if (slot >= dictSize) {
        slot = slot * base - twoCharBias + BASE91_INVERSE[input.charCodeAt(pos++)];
      }
      const value = single[slot];
      if (value < 0) {
        let read = start[slot];
        const readEnd = read + length[slot];
        while (read < readEnd)
          out[outIndex++] = pool[read++];
      } else {
        out[outIndex++] = value;
      }
    }
    return out;
  }

  // node_modules/entities/dist/generated/decode-data-html.js
  var htmlDecodeTree = /* @__PURE__ */ decodeTrieDict("!}.&u%}'&}*'~!6*)%&,~!J~!J~%L~y<~!R,~~%Lu~~#GD~~#|)1#%}^%}2%+#.##%##%}&%##%'#%##&%#%#'%#&#%#&#'#%%#&#%##%#)%''%&%#%#'%#%%#%%}%%%#%#&(23#%%#&-%0%('1#(##%#'##+%'*.:1}#%#6-+(%'%%#%%%}#L'2351&('%}&/N'(0(/*-%(%%}#'+&T%7.2}#&%&#%#36/5##%&%%#&#%%#))2%%##%&&'0~!#*+&'%1~!%).'3q?&%'1~!.##%6(~!+%%%(Gw'rT~!E#<nA%#jZ~!H%(~!42##~!*31&~!G%U~#)5~#`3~!J~!Z~%]~%Y~%C~!q~!u~#kz~%#~!6'~!D~!U~!?~#T~!c%~!G#'~%7|~!G~!J~!G&~#pb~(Df}#%}*&}#%##%##%##&#-}&'#'&%#.++}%mI,#,@&(}*%}*'%&##&#%##%}&0}#.},U},%}+%}&%}#%##&}B%(}(%}+%)})%##%#&}&%##%&}<%}>%#%&}*%}(%}9%}/%})%}*%}*%}?&}&%}3%}&*#%})%#%#)}#&#-#+*%E%%'%'#%}#*V##&##I}#&&##%&%#&&Qf%%))w/0+&%#(#.%-''''++++7}>%4'',##1,#%#&%##&#'##&#*#9)%&%}#*}%,#+P(%A&%#'&##wSD',9E00#y#@}(+}&%&>~!#~!X}#*}(&&}(&}(,%}%&#+&}#&}I%#%}%)#(},'%#*}4%%#%}(''}#/##(##),%-##%%)#&}(.}&%#&}%%}*&#%},&&}&%}#%*'#%})%}D&}&%}-&}6&#&}-,%}#%})-(~+`~,=?~I9'9%~!,#%})%})%}@%}?%}(~!?~#<~#pP~#BG~#=1#%K+~#?#~%;)~#A~#mF1~#A'~'X%'~#lR~#N~'N~#r~#m#-~#i'?%#'%~#B%##%,%#~#_%#0%~#]732~,w~2+#:&#%&'0%&>%}#>##F+)#%&&#(+_}4&}-%}(&}@&}O7Fdf0@+/v4}&WU##&/0#&'('B#%}.%}'+#%}#%%&#&%#%##+#&#)#6#'#.},%}c%},%#%##%&#&%#&~#>'*-.%##%##%}#%%}%'~#)D1}#%*&~#_%%'(~#S2%'.}#~#=##*'*-%}&'%'##&&~'E%.#&~#M4}%%##&'%#~#O1##%&#'+~#<B%##%%'%+~#;#@%}#&%#&&%#(~#H1}'%'##&&~#?A}&'~#D#%32}'&&&&~#[}'(#%}'~#;C})&}%%#%~#=&%,3}%'(#%%~#^'#&&)#%'~#Y%-~#d-%'~#^%%&#&&&}#~#b~2t*&'~&(~&@~0%~e~3}%*''0})&}+~!9##-}#%-hD*)1fC#%/&/fB#40~!+#)*4~!+~!K'&:~!/*7~!.#~!H~!L':~%x&~!H#~!*~%1~!I#~!+A~#p'~!F~~#-#~,,(~.Z~!V~%;'B'mq-W~!N~%I%#&&#&}#%},%%}'%}+X#%}#&}(%}'%}<%}#%}%%'}'%}:~![)9@~%>~#UA%-%##&~!C%~!-.9:~!1~!-^2/:a~!y,D*J#-5)/4~%23,~#G~!L1~!0X3`~!2+~!!0-~&E~!W~!o,>Y&]~%cZx_&~#O*9#A#'#+I'%#)~!0B*-5A+-((F&*M#)(-7-5+'-3a5Vi~!Y~!?+[)%3),ERHm~!+:D,VG.+)?fB%%*(%)'(#&80%1'8`K8?`+'Z#&O&'H5#*9)A%%5&3))0%39+.*7#()&&*=4@**L)<'_&*+..;(#*+)./&0#3)%')-8(4ixD(&.}%,('aI:,)%,k2231T)I'#/-W7,/'Q#.'Y24+h')37</31&83##&0#),H(?'&?/1##%#&&#%''-%&&&#(&''&#.-'%#%%(,')*'&#&#'##%(%(#%('#&##%%%%('%#%#%%#%#&%##h>w+v<ayvyvcg.uuhKr}g/v|g>u9i[~>g5uI~=RvdwEg;v/g;uk!!TTSx]@RT!U!#!@VBRUU!'UTe-d0c`e&gSdicedFcrdTaqb.kYcAohdYd@a3e+d}dMdtd.aJ#bqcK`dle/e.e'dwdPdodddjbEb}ogd^ofdpduc6j?l%d{drdqc)d7bacOdQ%T#Y)X.sR[yH>6Vyv3[xwLu>vo'!*.[yBacahoj>6Rew3[xqdZa#!a&#^(X-[yG>6Vyu3[xvg3sEr|g.u/Ri9db0T#^(Xa)!-[y;>6Vylg4wKs{JwNZt3@3r=c4Z([xlg;wKt!cpq's@v7A'*a(a+!-a#[y<3Dt?3Dt'>6Vym3[xmg9rxsNJwLZt4~?r?db1T#`-!(Xa,!0[yS>6Vz%NuQs.g4wKtnJwNZtS@3r>c4Z([y%g;wKtrdga8!a(!#&T*Y-Xa#!a0<or[yc3Dtq>6Vz43[y3JwNZtf@3s!Ju}!%Dti:pm3c_%X#tjB5pkd6q!r]u?voC'*-a.a2!0a&a+[yI3DtI3Ds~3DtH>6Vyw3[xx;:s#~<5pKJwNZtE@3r~d`a)!a2T#a.(!+U.X1[yT3Dt`3Dtv>6Vz&3[y&g9rxwzcxstPu.<rAJwLZtT~?r@dZa%!a.&^*Za(/Reu[ya>6Vz23[y1g3sEr}wkg{NuQRg{ci(U#5@b`~,cg#U(2WnH5wugcRh7dX#T(Y,a'Ta!!a,[yZ<]mj>6Vz,3[y+Pv#5ReZKu+=,%!H}7ABwkaS?Rh:BcW(X#<]mrj:ubv/ARekdg%!(!a.*Ta(Y.X1!#sP>Rl*Dt6[y>>6Vyo3Wf*jOvuumvuRgRJuq*!:9<B@bX~3jVv&v@s@5Re[d/rQt{uAvo&a&a*)a2!,0Wf!3Dt0=Bs'>6Re}3[xy~<5s%JwJZt1~Gs)c;&!#2sJkNuXvzq7rxu,Re8dka4!a8(aEZ+a@Y.X1Xa)[yd=Bs(3DtP>6Vz53[y4cX#X&Re:avRe9~<5s&JwJZtQ~Gs*i^rzvdRg+Jv{%!2sbB@bX}kdga,!Za?&^*T1/!a'Dt+[y6>6Vyf3Wf%g/u;s4hGu6?Rh-JvZ,!c%#&RoX54Rivj7uyvf8RgTKvZB%*!2sGh<vu5Rgq<=C::9bb~#dZ#T&Ta6Y.X*Dt>[y93Wf)coZ(T,6VyifluvRgC@95@B@bX~/hFu34cC#T,k/unq8w8Q5RkUklwQuzunq8w8Q5Rk8d/rJu?v8w9)-&!a0a;a&aIWejg3sEr/h1s<DtDJvyZqY5aws3Jvy!&Wei~Hr1:au5@Bag>23E~5c:Z&bX};kKv?w&unuVu5Rjc;>bs)#~@:Rh.=ay<a]C;b`}Vd6s/t{uAvoaxa()!a,a7%-a#a2Dt,[yF2Wo[>6Vyt3[xuNuPRi&NuPwpi#RoWh?vf8Ri%Jv]!%Ri:KvxD!.'2WeAjZu`q9rxu,Re7woeAg-unLq(qA_/*2Wg_g3u5q^9:4E}/jTrxrzv=Wkkd~0UX#^^Xa-a1a5T&a=U1a'*aEa]!a*aPaA-adok[y54Rn>;:p3~Dp5g9rpsFNvZqjg3uJp4~<5p0Pw;5qlJwNZt*@3p1Pw:5p/Ou!5p2JvG'!6Vye=<qnJvh_[xhg3v,Rh3kOwOw-sDuev/Re^dha[a%!%!a+#Ta7)-5TaCaO!aka!a)sf[yb2>Rl!9ARiq5E}Qg=ucRkBE|oJrJ_@Wk~@Wk{JrJ_@Wk|@WkyJrJ_@Wk}@WkzJvO_[y2g-vMRmiKuYC!)&>Ri;>Ri<@3RkNc](X#@9Rk=g5vuRmhKvDB!+'=]meg3u4Rmgd)#Y'Vz3CARmfd`a+!%T'!+#Ta1Ta6TaM-sTDt9[yA9sYd'%Y#s[[xpj:ueunaXRgEjRq,v-vuqdd2'`#6Rev<32@5>:2<E}5xIo9a*X#Y(;5RePJvD_g>vyRgNj8w)v8<wggs:RgXiZt|vjx,hSq3ah!-(~@:Ro/Ou!5RhWj^v(pyw8unRhUdx-UY#^Ua.a3a70!)%UX1TaDa)'omRiRRhE[y:3Dsz=Br,>6Vyj3[xkg6ruwjcqsrPw;5r*Ku]D'Zt-@3r(~?r.i[vwv]dU1a--U#`a4(g/vsRhPOu!5RhLj:rmu9Wo!~@:wdh@g/vsRiTjXuvvNr}:RhBj^v(pyw8unRn]dz1UYa'a+^Y(!aETZalaRY.Ta?a4[yDJw1!#qLsW>6Vyrfzq-pLflpwRe|Js>%!Dt@3Dt&Jvy_[xs~HrnjMuwpsw'RecKu+D#'!t<~Grl~?rjg5u-x,gwp{ah!-(~@:Rg~Ou!5Rh'jXuvvNr}:Rh#cW#X/c;&!#2sLi[v7u7RgpJv)(!iLrxu,Re6j7v@s@5Se[e7d`aW!Za(a`T.a#!a3!&aDa-!9)Dt_=6s+3[x~~DR|h~DS6avhGun5RkZj3w)v-]mkKunB!&*]kb97R|i<ARk<c:Z(6Vy}Juh'!wziMRoS:F|vkLuauJv5vtvQRh1d='T+Y#VyO~DR|jcF#T'7R|g97R|kJv3'!ay<Rj,Jvh&!:ReXcsa6*a+#a#_aIRf9aLRf?c,Z&Rf5Rf7c.Z&Rf;Rf>cQ#%T'p-Rf8Rf=ct#%'(*!,p,Rf4p+Rf6Rf:Rf<d~'Ua%U*^UYa(!a,-!#a4YaTalaEX0a8a<Weo3Dt/3Dsx=Br93Wen~Dr;~<5p<JwNZt2@3p=Pw:5p;Ou!5r3c7&!#:p>3Ds}KvGB)_6Vyk2sM=<r7x'eovA(!hFu1ARf}cV#X&@r5j6rvwQa^Rf3c=Za'wkghJv__g;unRggA53B9=b^}%j6uduo5Jq;!(hIv%2Re`Ou4ARe_e%a#^^^Xa&!a*a2!&a6YaP!*ad!#a:aE/5Rn?[y@>6Vyp;:pE~DrY~<5pBJwNZt8@3pCh=rt3rWPw:5pAJup_[xoNuPpF9c!#'45pD5ARn)d8#X'X*3@rU72s]h>v<<sSjJpqvewOJq/(!hNw'5ReBk0s2u3w/w'5ReE5@Jq.!a+JQ!&WeU23d(#Y&RjG5]jBk!u7w&u0udARjEe#+^^^Ub#!a2/a`Z(agT1!a-a;|@TaG!aS[yV=Re~fow'RguNuPRe?bz#'>RoUWeL>:Cbb|?JwPZtVg6ruRmzJvD'!6Vz(g/vmRh~Jvy_[y(g9voRgyx*cy(#2>Ri2B9b]~9kIw9u7rluJu3Rg]dI#a%UY'@=p%CAx.gQZ&RhwwygtRm{x5g_Z'+ABqR9Woa=Bp&dV#^*Xa'!&@o{g4v]Rk;Jv{!%Rk[wkkiA5RkiwwfUB=x,fUuqC&*!>RfTg8v0RfV~ARfSd;rJsAuAv9wR'ae+/aO!a@aza/a#[yQ@Wg!2Wemg3sEr0JvB_g>uvReWg2v+Re=KupB_+[y!2AbY~-~Hr2AJwD!(h<~El>h<~El?Kun@+_:9b`}Kg-v/Ri3g;vtwyk_9]k_d=&T#*U.6qh@Ab`|K9:H|CJv[!&3Dtex'fDwC%!Rf[9WlMd[(^X,!a%Z06Vz!@WgBg=v~Rgvg,QRe@awd,#Y+jTv|Q~EfWj]uNr|~FRfXdy#Y&^Ua%!aO.!(a)Ua;=!a@aKap!a-,a!Ta]a[rSa]p?[y82sK=Bq~;:p:~<5p8Pw:5p7d'#Y'Wf(;RnRi[u4w&RgJJvG'!6Vyh=<r#ijuuv/sIKuYD'ZtG@3p9~Gr&d2#`(g<vtRgFj`u5w&rqpxRf2CJuY!+:wfnTOu!5Rg}jNs1ucv&RfwJvA!&3@q|BDcC#T,k/unq8w8Q5RkTklwQuzunq8w8Q5Rk9dga#!a'!a=#a0!:+Tb*b@aO.a4!aba8aFJv^}?!VyR~Dr<g;u%Rn.~<5p[x'e`wNZtR@3p]Pw:5pZhNvjBp.woe_g5u-r4JwF!%DtO3:ooc7&!#:p^3DtpLuGw(!+%)Dtk6Vz#2sd=<r8d'#Y([y#<x3gJt`w@!)%}MRiowzikRij=]ilxAf3,U(#B2Rf#g0v-Rm[ck{`U#]giKv3>)!&6Ri154s,KuGB_%@r68r:dJ|t`#X(9<E|u2@H|rx3gJu?w'!+'1Nu7Reg4=H~+9<wxgY95Rm]xLggZ-`(X}U2:Ri4h<uOawRmsJv__5@bb{jbV~3dka#a'a]!,#a+U=a>b6a3b%!/aKa/)!arwve^VyJ;:pR~DpTg3uJpS~<5pOPw;5qmPw:5pNOu!5pQJvG'!6Vyx=<qoJvA!{~Jup!%@qk7Rn/KvyD!}''[xz;>wkh'?Rh,x8gyt`w5D!&),(SgyccRgztJ@3pPB5p#d'(Y#<]mmifubw&RgoJvE&!82s^JvF&!8Rf,ADb]~;x=h'rNu]vK!,%'*0RnORh)4Rh*AqQg-vaRnNg;wHwkh'ba~4cE#Ta*x3gctyw@'!+%RnFRnD<4Rn@hFvK5RnCxWg[#`&a0Ua()`1Rm75Rg[c]%X#qi8Rg^NvdRj>BwzgZauwji7Rm6A4wgg]d1#&(*,.0a#Rm;Rm<Rm=Rm>Rm?Rm@RmARmBe%#^^^Xaea?aC/b+(,!a+a#!a/!>a&Ta<aKbD!2wphBRnk[yPw}hE|.=Br-3Dtm>6Vy~g6urRf.x,hPrNav!%'RnqRo%Ro#Nu;q[Pw;5r+JwNZtM@3r)d'#Y'Weh;xChL#`&RnmRnoKu}>%(!Rne~Bs-;2wjcussJv+'!aYSO}6@B<5?ba~8LrNvj!.%*ROwungw~ng~:9;Ri^>wtnig;wHRnixDh@|(UZ.x1h@|)!#:2<H|*xHn]#-UX'3Ro)z=iT}6ARns=Bwsn_wpnaRncw]aR(#UXa&Ua*a/=]iPd'#Y&Ro'WnXf{QRm2hNvj]nZd`'T~&1`{|`#9b]{}c:'!#Wl{>@=be}]?cl{{U#:5Abb}Jds#^YaF!a*b4a#a3aPa>&Tb!bH!*a_!Eau?/a&RjY<]gj>6Vz*;:pe~DrZg,QRj1JwNZtX@wihspcJvZ&!VyX9WmOJu|!|N2WmHJvh&!]ht~Bpbcn&T(!#RmQ<s7Nu;padH#X'`+WmJ@>RmKCARhnKup=!)&Wf+:RhqNuPpf9c!#'45pd5AwghpARn(Ls@w!%,)!RmP@Wfe<E|IJva!&WmNg8vsRmLd`*.`#Y'Xa!axRn*]hrA8Rhug5s@rXg8u!RmMd8#X'X*3@rV72smdI*#UY&RmICARho~GsgxVgd)Ta'U-Y&Xa!T#RnEWnA@Wffg1uDRi0hFvK5RnBxGnG&#`%owp)@wsf+bX}Ze-*1!a*^^^Ua|!#a.aq&Ya2!a>.a6!a:aO`aJDtL[y`@Wg#>6Vz12@wzoYRoZNuPRi!NuPRhzg=ucRi,@=b`{Yg=ucRi-ACJvB!&Sh[ebSh]ebi`wUuFRm4Jw2_[y0JvB!.<Ju(!&SoG}6Shd}6<Ju(!&SoH}6She}6Kur@._g5vHRieJvx!{L2G{Kx6gd'T#?Rh82Wi5cZ#X(g1w)Rm5dW-Y(Ta#!a)!#aYa=wnfE=su2>>bU{0j9udv:<svj8uQv-7RgHdE%#^'sq9sp=>Bb_{TJv`!&g/r|snj6v(us5d,#Y(56H}[978H}]Jw5!&g1rushJvB!+j;v{u5?zDhd}6}bj;v{u5?zDhe}6}ce*#`(^^^a[aea!=!a6a*aoXb1a.!aAbL!b>,b'aL!aV@Wf|2Wlg3[y/JwNZt^@3piPw:5pgJunZou3@rsJva&!Vy_g<v~Rm#JvG'!6Vz0=<r{Ju{%!:pj@WfsiXuJu3Rm:JvZ&!WfA~Bph@c4Z&Dtwax5rubx(#:awRk1@d,#Y&RfjRfid1#,Y(@Wfp2Wlrg5s@ryKu[@!,'=]ig9wlk?Rk>g5u-rqJvy'!@9RkQcH(T#=>Ri~@<wkj(Wj(KuZB*!&<7rw@9RkRcH(T#=>Ri}@<wkj)Wj)dg(Ta2Xa9X#`-!a*CARhg@@=I}d9x;c~#X%so=<sj>2@@=aybb}XjWv0Q~EfEj3vLv;<d,#Y(56H}`978H}_dgaPaFa'a/!#a3Y0a_a;a|!1(a7-[yE3[xt;:pJNvZrrg3uJrvJwNZt=@3pIh=rt3rxPw:5pGOu!5rpJvG'!6Vys=<rz@c4Z&Dt(ax5rtJvZ!&~BpH@wsfNg-vaRlNci*U#=<wei<F}a5@Jq.!a*JQ!%@qZ23d(#Y&RjH5]jCk!u7w&u0udARjFd/prq=tyvpaEa(a:.!a1aZ(@@=I}:9wpd%=<sX55w_h}@@=I{t=ay<aU@@=I}T=ay<2@@=I})?C9:9au@9Cb]}DP~=x-fAZ(2Wl1=ay<aU@@=I}>5@d##Y+jTv|vV~EfFj]uNpn~FRfGdgaK!Z2&!a8a-Tb({E!acTbM*!a(DtY[yYd'%Y#sl[y*hHvh>Re5x2c{Z}.j4uCvcawRiMd+#X+_x&d!},<5RkX;2Hzw@x,gavfB-!{CcF&T#Roe;RodwWbBg5urRgaKvHC*_6Vz+<4opieuew&Rmq@d]&Y)X,T#X0Rh}<BqP=4qS9:ReMg/ujReNJw0!/<Jui%!bd{kawwnemRelAxUa?a3#*.&UX(Ya+a/RhvRnQ<o}9Wmtd-#Y&RgSRmw9;Rmxay=Rmyg-vaRmuxEhSrNu,v-voC!%(aR.a(a7+1Ro1>Ro5CE{A9b]{@;5x#eO{:g;urRi+KrNA!%(Ro3>Ro79;Ri_Ku@>{;&!x%gX|{KunA_+g5QRj/g3u5Rj#g>uERj%wio/xRhS&!,!#^1U}wba{8>>@=be}qC@:D5ba{7Ku+A&!}x?ba}t>>@=be}se(aA^^^Uat!b0#{pa+awUazbGa#aLb9bgaWac'a5TbS=Br!d1#`%scp_Jvl!#rT>Re0JvX&!VyN=H{Fcm#U&:pY=ReaJv2&!]h0=]nUJvG'!6Vy|=<r%JrM_=]h2@Wlud'#)U'Wf'b]{i=]h/Jvh!&~BpWg=v]RnMx+ny#'Nu;pVwjnu=]nwxJnx,T#`&Reqwjnt=]nvieu9vrRjLLuYwP(#+!th@wih5pX~Gr'g5v/Rh4KunA'!-CARnP@wwiN:Rm_9x'cvw>!|l=<saKvAA!0&3@q}>w^e1bp#&Re2Re3BDx7gH#T|f5H|eKuZ>!%(:qNAH{]Jv6!+3B2B9=b^{X<5<B92:E{ZLvhwA(a;a%!igQuyRmad+#Y}m@3Rh5d8#X'X*:AqUAHzmaxwbh<aXRnVcF}RT#Nw&cj#U(BWnug/vsRntdka)(a3+.Zb7aYYan1!bVa@Xa}[y^@b[{G=H{+hFu73Rj&Pv#5ReQcK%T#sig1v{Rj'Ku+D#'!t]~Grm~?rkKuMB!01d5#`'Vy.ta3Dtu~Hroc8#'{^45s85AwZbP&!#Rn!wghxWn#KvEA!)&2RlA2RlBx:h|#(T,=]j09Wobz>x]z/@awRoTd+#Y(az]hFhCrm4d,#Y+jTv|Q~EfMj]uNr|~FRfOdCa!Xa9_X#@<plJvf!%b`{(9;Rgwc;.!#2x7cw#T|UDb]|T5Ju={(!=@E{&Jv)&!Ab`{'awJvf!~*>>@=be{#KuY>!+&4Ezyi[ugv&RjIdea+T)#UXa&T-T&a!Rh9auRmW=]kLg5vuRn+g3u4Rn-Ow6ARn,hHus5xNk?#UX(U~)/g8v0RkD~AwkkF?Ri.OuNBwkkA?Ri/d|a2`a*^UYa.!aBTZaTa'Xa;!(!2!-a#b2[yC>6Vyq3[xr2Wi?g1rusVh%s?DtF~<5rbJs;%!DtBfswKtCj[uvuSsEu3RgVx3o:u+wN'*Zt;@3rd~Grh~?rfg8w)Lq)qE&-a%!>bI|`jWv0vV~EfCjTv|vV~Ef@j]uNpn~FRfBcK#T']gWNu7x,k7q4ai(0!hHv8<RhmkMu9vrsBuev/RhlCJvB!,g<v{wchh~@:Rhji[vrv{wchi~@:RhkdS&a5UY#Ta!RgPwwiI5BwciI~@:Rh`x'iJvj'!5]iJPu8Bwch]~@:Rhach)U#h3rp]gLh@t|Ax,hTq3ah!-(~@:Ro0Ou!5RhXj^v(pyw8unRhVd|)`,^UYas!a?/a2Z'a^Ta{Tb7Ta(a#!a,Wf&9sZ3DtAadamov=Bqt3[xig8vsRm~>waiL2b`{QJv*_Ouv2qgj<v]v2BqfdR'X*X#Y-@3qr~Gqv~?p6hHv-]glPup5Lq+q?_%*b_{qF{n9b^{rOu4ARhpKvCD!+&~Bqp:5Dbb}nwoiKl&unuTuBv]v+ueunaXRf0=Jvh!0nKufu8v1w&w7q%w&uHrz:Rgnj5w,uxDJq/(!hNw'5ReCk0s2u3w/w'5ReFd>Za&!*UaA=<wkgsRnSJv^!%Refifw3vyRgOKu_B'!,<]gkiiu:w&Rh<=C@a^<B57@2F{[<B5@aW:=3away9A5aW=<B=C@a^<B57@2F{Ie-#`(^^^bCara.b8aza6!/bZ,!adTbnTbOb+aFaS!aAT9@Wf~2Wli3Dtl2@d,#Y&RfnRfmJwJZtN~GqyJva&!VyMg<v~Rm%iXuJu3Rm9Jv[_=]ih9wlkDRkCd1#`(@Wg>2Wls3cH#T(@<Rj*=>Ri|b~'#23s9h<~El.d'#Y&Dtxi^rzvdRl#d*#U%(o|B2s`hJwSaxRmDKv4B&!1:Rmdd5#`'Vx}to~Hq{x'f1v3(!BA5ba|bJv_&!Wfug1v]ReIdO+U/Y#&G}-8wze=Rh{g1v]ReHg/uQRf/by#)ibQwERl/cH#T(@<Rj+=>Ri{cNu+vlax-!(#a0qa9<Rii2;;bU{H;x<i=&X#Rk`<4wwi=C9H~8xAI(Y#<azRi@45wXI<B9;5bb~7dL(X#Xa(+!aL6Vy{g5QqOau:5au2@ay547EzbxOcU(UX-T#Ta#:Cbb|A?wjh/b_|SOw6ARgtihr}u7Rhy<d1#T)X1@@=I|~=ay<2@@=aybb}Sj3vLv;<d,#Y(56H}A978H}@dGpvs@uAu`vcw9*!aFa+ai%(b!aXa8.a?a[ozWey=sU2@G}Nch&U#Rf_WexKu+D#'!t:~Gr`~?r^j]uNr|~FRg*j^psurwJt|RmcKv)@&!)7Rkv~Br[@wxfO:Rl3co#U'6Rezj_q#vIuavjRltwzeyh@vr5JqD0!>aY?C9:9au@9Cb]}9cl#U*5;5<H||jbuus1ucv&Rfvg1v~d/pppzqFr^a--a~!aMat1(hFv;Wiz@@=Izoj5uuv-7Rix~Cw`fk2WlVcZ#X,k)u3vWs@u2]ktg;wEx'fBq(_2Wg/jTv|vV~EfoJv]!15x'hzqG!(P~EfU~CRl_j6v(us5x4i-#T(2WmZ?C2F|d>Kq<aj1!*jTqIsBv=Wl`~Cw`fi2WlWj`v0u*~>RlR=c>Z,k#u3vWs@u2]kr<c1Z+jTqIsBv=Wla~Cw`fm2WlXdmb3!a{(arZa`bkTa%TbQTa-a9+c'!aM!/[yL=Bqug.w'RifhFvyDRj.g>vgwyk^9]k^Jv3_@WfbAARkhJw2_[x|JvB_wkoIRoKwkoJRoLd'(Y#<]gm=<9<H|yd'%_X#skDtb3awwqkgNulRkgdB#^',9:p'hJwSaxRmEBwVb8@4=H|qLu+w50&!)@3qs~?pU>Awwn;;Rn=c:Z'ARn<=<qwKvC@!/&~BqqJv6!&]eVb^z^xRge'/a%+^`#Sge}6<4Rn3=]n0Pw2>Rn8Jw0!&>Rn:>Rn6cY#a7+!a&=<wkaNw~h3z_c5Z{=wjh#=]nLKv^D!&)Vyz=bW|swYb<WetcG#T(2wxa@qVx@gD#Y&b^|V5JwG&!5bb|pg/w&RgD@x=kHs=uAvn!a%%/'+RmSRh694Ro`g-vaRmRhHv-]mlxCcS#`&ba~.5cD#Ta)P~=d,#Y(56H{>978H{Dd_#{2^Y%_+qbbb{6g3sERhsbU{?dfa.,`a(Xa<!aiX#(55RiG54RiHcI#T'WiU3RiVNvdwtfcRlKNvdd,#Y&RlHRlExQgf.1*^T'X#Sgf}6Wn4=]hfPrk>Rn7Jw0!&>Rn5>Rn9Lunw?&a2!,5<oq@@wqfdRlJj5Q~=d,#Y(~ARfcOuN]fdDKw;ay(}i!547E}j?cI#T(@5bV}iCbV}hdv(^^Tb?a40,b##Tbo!a*bR!a<b|a/!aKai!aU[yK=]o^g:v>ReGJwPZtK<7Rh+h<~El,Pv#5ReR@awwxjCg,ulRjDJv6&!]j!z?aQeeg>w=Sh<eeJw;!&axEzOg,Qosc!#*:wkeJ]eJ>x'h-u(!%Ro.w~h.zPdNZ(X,Ya![x{;9ReY;wkgxRiF:x?ap#Y&RmUg<s2Rkod]+UY0TZ'!a&A9sw<=bczLNvuw{gqzNhJwSaxRmCKuLay!#&s_Rf-55b^{uJvZa!!c%#(55Ri654wmiu5RiuawLu,vp!+}^%b_}Y9;wkgxba}o>A9:=b^}zKuh=a''!3awRk3c*'!#aHRk6c+Z&Rk5Rk4Jv)&!awRjSawd9*`#0?C2@EzMj8u<uJ5RmbjQrquJu3x,k>uq@_+=ayb^|W~ARkEOuN]k@7dhzV^X/X&a-#zRzSb`zXcJzTT#2WkVKvDBzW!%FzY9;5bbzWjQrquJu3Jw3%!b`zU=ayb^zQd:#X(T-a!6Vyywxh}=b]{Jg=u1RiAdGp~qHtzv!w(wA+a+a;<!aJaYai'anasb(=azRmV:Cbb{MLq2vb!%')RjuRjrRjtRjqx3jnqCw3!%')Rk(Rk+Rk&Rk)Lq2vb!%')Rj{RjxRjzRjwLq2vb!%')RjsRjpRjfRjex3jcqCw3!%')Rk'Rk*RjkRjl9<CbbzfOu4ARhxLq2vb!%')RjyRjvRjhRjgx=joq*uKvb!%')+-Rk.Rk%Rj~Rk-Rk#Rj}x=jdq*uKvb!%')+-Rk,Rk!Rj|RjmRjjRjidAq&qKs@uAv8Aa.'*-a@a&0!aM@a5[y73Dsy3Ds|3Dt):wxgI2sHJwJZt.~Gqxwsf0ikrzt}Rl0Jvy_[xj~HqzKv_A|D!&WfP8axRoVcf,U#k(v]v+ueunaXRf1Ju}'!g8u#Ri=jQw!sCunLprq>!,')~<5qeGzq9F{W=c##%s5au:5aU3CBE|;d4#X(D!a&6Vygx(b;#(=]ed?C2F{N<capoq2r[a&!aPa9,'Pw;5s:@@=I|,55w_h|@@=IzcP~=x'fCqB_2Wl2>aU@@=I|1OuNBc1Z+jTqIsBv=Wlc~Cw`fl2WlZ~AcTa%!Z+jTqIsBv=Wlb~Cw`fh2WlYk+uNqJsBv=WlSg,u3dca3#UXaMYa)TaB-=cM|7T#<bI}l5@B932:aV2G{BOuNBJq:|M!5Ezt=<B=C@a^<B57@2F{v>cB{/T#=ay<bI{3Jv6!a.6BKq0ah&+!5E}HP~Ef{978BaU@@=Iza<7d#.Y#978BaU@@=IzH~AJq0!(@@=IzG978BaU@@=IzFe,aU*Y&^^^bvJb,b:bFad!a,c2Ta>aL.bo6!a#CbTa'T#Re{2Wlh2@G{yg6t~Ro_NvdRfticuRQRllJv3&!x&c|zs@Jw3!%RflwpfkRlpKuL;%(!Re<@G|C2GzdhIvuBwgjAg-u0RjAKQB%!(GzZ@G|5NuuRl7d='T+Y#Vy[g<v~Rm!==G|>JvA!)@wma=]m1ifuaw&RmnLs@vT'!|/+[y,g:v>ReTJw1!#qX=x!eC{bLu+wT&)ZtZauq_~Graci&U#F|89:r_Lupvq!.)&2RlG8RfaC=x!eF{_h?rpWlmd&'!#X|&]k::xJey#`'T|+<E|&2@H|%dE#(^,g;u.RiEg6vjRiC9xCkA{O|zY#g=ucRmXKs0@!&*@G|m@awRknJuh!,3d(}gY}eJvj!%Rm):Jw3!%Rm+Rm-Ls0w(&!a(a#@b[|6cZ#X'7RkxWgAOu4ARn'dH'U#Y*Vz-Wm'CARm}d]*#a%^a*T'aK!a<9bV{PC=p*Jw4!&SgxcbB5r]idw(wBRmF7xFkt#&`(Rm/Rm8E|!JuY_9:Rl5=wrgr2:bbxd@xXfB(a*#T+!.X0X1Ta/a'T&RlDRfL>RlyARl9b[z[>RfZ:RlL:RfRwlg/ARl;9;RlxKv,A/!%7s69<74=BA5ba{-8Bde#`a<XaKYa1,a'P~=wxfB2bZ}}?C972@@=I}r8@55B9;5bb}G978B2@@=aybb}3j3vLv;<Jw3&!>Rfk=ayb^}4~Ad1#`*@@=aybb{w2@>==<bbz]dx+UY#^UaF!a9!bB'Ya1.!ajXa#%olRhD[y=3Dt#Ov5BrHKuMB%!(Rf^Wep~HrJwkiQjKr|~FRg)Ku+D#'!t5~GrF~?rDdV)UY,Z/_7RkuG{<~BrBg,rlsO:235B@bX}|d?a1!#`(6Vyn5@d##Y+jTv|vV~EfIj]uNpn~FRfH7Lq2vb1!a9-978BaU@@=Iz9978BbU}#~AJq0!(@@=Iz8978BaU@@=Iz7~AJQ|}!978BbU}!JvkaK!AdUa21-U#`a+(g/vsRn~Ou!5RPj:rmu9WhOjXuvvNr}:RhAj^v(pyw8unRn[kPr}p|u7vwv]RiSBd;pppzq@qHQa?(b.!a.a`@.|xa(hFv;Wiyj5uuv-7Riw~Cw`fg2WlU978BbU|wOuNBJqG!(P~EfD~CRlQcZ#X,k)u3vWs@u2]ksg;wEx'f@q1_2Wg.j]uNpn~FRfqJv]!15x'h{qG!(@@=IzK~CRl^j6v(us5x4i,#T(2WmY?C2F{1>Kq<aj1!*jTqIsBv=Wld~Cw`fj2Wl[j`v0u*~>RlT=c>Z,k#u3vWs@u2]kq<c1Z+jTqIsBv=Wle~Cw`fn2Wl]dn1#c(a(b^a2!b/bAT(bj!aDa7bu,a_a{c0!2T0g:v>ReD2@G{42@G{5~DpM~<5rc=Bx6i>{RT#RnI@zCx]y]z:2Jv[!zr5Awyk]9]k]dD(Y+X#6Vz.g=wKtgwhaCwgmTWj2Lu,w%_+/[y-B;b^xeg3u3Rj-2@bX{*KrJ<!+'@Wg(g?QRlC@Jv`!%b[zIwsfII}8JQ_@w|kW|=Jv(%!AqcOuNBJvEzh!bYzjLs@wP#(0!oy@>RkdJwMZtc3Dtd@BcG#T'9bWxg2@2Fznd*#Y+;2x'c}w<zizixNgwa#Z'U+!/!a'!a+w~g~z6wcn{Rn}wcnzRn|5Rh%=]nJg5vuRmvNvdRlvcprJu}w*az*a#!%.a.'Bot9qT]kj@Wg'ay2Gzv@Jv`!%b[zEwsfHI}1;ck#Ux`<Cbbx_Lu+w!a&0*!wko*wwo,So,}6Juqxf!E}PigQuyRm`d3(`#8>Rn%:A5B;bZ~%KvhCa!a2!x>k7#Uxb@b{#xaRk7Jw0!)>wwhlShl}6>wwhmShm}6CJvB!.x'hhvj{!!5Bwkhhbaz}x'hivjz~!5Bwkhibaz|xEhTrNu,v-vpD!a%&/)a3a.,%Ro2t[CE{)@3re9b]{%wjo09:rgc:Z&Ro6=<riifuaw&RmoKrNA!%(Ro4>Ro89;Ri`dSaL'UYzxZb)7Rka3xRhT&!,!#^1U}vbaz{>>@=be}yC@:D5bazzKu+A&!}{?ba}y>>@=be}wxBh[t`u~vJvr!%a!a()a,a0a4RoC=]o;Ju(!%RoGRhdwjh`=]oAg>w#Ro?g5vuRo=NvdRl|Ku]C.!&;RoEJvB!%RoORoMBx'h[v+_?w~h`}~5?w~hd~!xKh]oiptu-utv.vp!#%&a30a@a'a+(a/aOp(o~p!RoDJu(!%RoHRhewjha=]oBNvdRl}g>w#Ro@g5vuRo>c[#X']o<CauRoRAd-#Y':RkpauRoQKu]C.!&;RoFJvB!%RoNRoPBx'h]v+_?w~ha}t5?w~he}ue!/UbhYacXaW^Tc&a;b:a-c/#b&aja1(!cL+!bKbt!bmcRc9aIc?8[yW3Dtt94Rg`Jv}!&SiRMzBhEebShEMNuPRe>x7gL#TzuwjirRipc<Z&>on;>z=h-MSh.Mwqczx'a7vj&!>Re4@=ResJt__NuPRi*NuPRi)j]uNr|~FRfzKrJ>_+@Wfy@Wf]2WocKrJ<!+'@Wg%g/QRl@@Jv`!&awRl<wsfFIzgLu(w*!.*&ShBMwvhIRhI9;RhNx1hK'!#Sn]Mx1hK~0!#:2<H~7cNu+w7D*'1ZtW>Rn1~?rOc:Z&Rn2=<rQ<7wjh&=BSnLMc]#X(6Vz)w[b=a!U#9wzgMc3#&(RgMRitRis<x,gKt`ax!&+SioM=BSilMc3#&(RgKRinRimKurB,!&SiQMzBhDebShDM6BJQ!(P~Efx978B2@@=I}WLrJw!!,a*&@G}O@9wkibRid@@x'fKwC!&SlDMSfLMjUv~Q~EfKKv3@a+!(hFv-]mpx/hYZ(C5RiWz<o/MwkhY?So/M@x,gbvfB*&!SgEM:SoeeehFu3:Rgbda(,^TZa)X/7Sg[eb:2RgI~BrMC@wgkc:wwkcRerx3h(uUvK!&*,SnOM4Sh*MArRg;wHRh(x=h;rJvPwI!a4',a'0@Wg&=BSh/Mg>w=Rh=g3w*wwgGRgGcW(X#;Sg}M2Gzk@Jv`!&awRl=wsfGIz`dKZ*T'Y-:RhR7RhQg5u-p`j6v(us5d,#Y+~Awkia?RicOuNBwkibba}Ld6p~tyu_vbAa'a+!a/'a3aEa8a!>Sh,ebJv{!&Sh@ebSaReb9;SgwebNuPRi(NvdRl)NuPRi'hHu^<Rm^Jvv_@Wl(g;u1Si/ebKu'B&!*Sh?eb@Wl'z@aPeb95Si.ebcpputyvjB)!,&a+0a%ShAMWeK@G}C@WfJ9;RhMwvhH9w{ia}ix,hJvRA1(!zAn[MRhHx1hJ~*!#hFv(BSn[MBJQ!(@@=I~'978B2@@=I}2db.Ua<'X}+T#a0XaG2G}E;wkg|wuh!Rh!x,hZu,@)!&So0MVy)C5RiXACJvB!&5RiY5RiZg8w)cG}*T#2@bU}=KsA>(!a.3wkhZba~(x,h^u(A!&(SoCMRhb5Bz=h[eb?w~hb~6x,h_u(A!&(SoDMRhc5Bz=h]eb?w~hc~6e)aA1T#T,^^^c-bMb&blcPaP(a/!0!bA=b5c@a(!bfbrc#2afwmhARnjwchORnp2Wlf3DtsNvdRl-2@wpa<]m0bx(#:awRk2@Jw3!%RfhwpfgRlnKQB%!(G{V@G|'NuuRl6d='T+Y#VyUg<v~Rl~==G|<Jv+'!aYShC}6@B<5?ba~8@Jw3'!g2QRljhLrpWlOd+#Y'g.w'rIg>w*wgj@g-u0Rj@Lu+wT&)ZtUauq]~GrGci&U#F|39:rELrNvj!.%*RhCwunfw~nf~:9;Ri]>wtnhg;wHRnhx3hDs@v~!/+'@Wfr@9RkSNu&Rlo=@<5GzoKs0@_+@Wl+@awRkmJuh!-3d(}pY#qWJvj!%Rm(:Jw3!%Rm,Rm*de&!1U-U#`)Re;@G|.@9Ri82@wjfvRlq=@<5GzpLvOvr!).&2RlF8Rf`C=x!eE{.Jw3_g2QRlkhLrpWlPde(!#U{s,UXa*Ta'[y'g:v>ReS;x0PZ&RnlRnn~HrKJw1}f!=x!eB|2w]aP(#Xa&a*Ta.Ua2a7=]iOd'#Y&Ro&WnWg;u.RiDg6vjRiBNvdRlzhNvj]nYJuW_2Wm3x)kFze{9d])!a.!,Y01!#&aC!a3RndC=ox~BrC@2b^{pg,rlse7x'ksuq!%Rm.E{xidw(wBRmGx9o+)X#wwo-So-}69:Rl4@xSf@a#XZ'X)X,Ta(/ARl8b[xc>RfY:RlI:RfQwlg.ARl:9;Rlwdn'#^XafaQa1X1TaHTa)@b[{zcZ#X'7RkwWg@Ou4ARn&x)kG#{,g7u/RkGdH'U#Y*Vz'Wm&CARm|bx#(A]gUbUzJj9Q~=d,#Y(56H}l978H{U7d,0#U*2>ABb_xZ978BbU{e~AJQ{g!978BbU{hxMh?ad{oUYZ.x1h?{l!#:2<H{mx3n[t{vl!,&a%3Ro(z=iS}6ARnr=Bwsn^wvn`Rnbd`*T}B0!#^X'BG{c9b]{a>>@=be}F?JvS!&BG{d7BG}(Bde#`a1X,Ya@!a'P~=wxf@2bZ}I56B2@@=aybb}08@55B9;5bb}<j3vLv;<Jw3&!>Rfg=ayb^}&OuNBKuLA!)a!P~=x#fD{f2@>==<bbzl?C972@@=Ix^d6rSu,v7w*C(0a)a6#B+a%!sQ[y?3Dt%3[xn~<5rLOu!5p@Ku+D#'!t7~GrP~?rNKvlaya7'!h+v-5qMg=t|cd,U#5AAaa5Abb{S@52B5@a[@52B5Gx[iXueu;d<#`a(!/549C;ag>23ExY5@Dah89b^~689Jv)!~2b[~1Lv'w(%*!a#bX|aPrmawRe]keu7uhv-q6rxu,q`xTo]/a5aU!bNaDXbi!b-!ao!b<bwA!#5@B932:aV2G|:d-)Y#hJrL>RhG<7@C5<H|_=Cau:5aj5@B932:bJ|ng>vIbs)#?C2F|9jPv0w.vISh-MKvUaz(.!9ABbb|[5;5<H|Eg>unwfh;9:4E|YjQsBt|vjx'hYq3!(?C2F|J:2<BaY?C2F|GOu!5x,g|p{ah!-(?C2F|c9:4E|OjXuvvNr}:Rh&i[w*t|cd+U#jJvsu)vsSn~Mkfrmu9p}u7vwv]So!McW#Xa!ax5@A5aY:5;5<H|>kJv~vYrquJu3x4ib#T)2@SmZM?C2F|Bj:rmu9@xPhI(a*a#U#`a3-5Abb|L~@:RhK9:4E|0@52B5G|#C::aY?C2F|-:2<BaY?C2F|.5Jvk!a)javYrquJu3x4ia#T)2@SmYM?C2F|HAxPhH(!a#U#`a*-5Abb|4~@:RhJ9:4E|R@52B5G|F:2<BaY?C2F|Sc^#Xa2j=Qq5CJvB!-g<v{z;hhM?C2F|Zi[vrv{z;hiM?C2F|XKsA>!a)-g<v{z;h[eb?C2F|]i[vrv{z;h]eb?C2F|^iZu.vix,hZq3ah!.(?C2F|QOu!5ShXM:2<BaY?C2F|P", 13494, 2713, 49, 25, 61);

  // node_modules/entities/dist/generated/decode-data-xml.js
  var xmlDecodeTree = /* @__PURE__ */ new Uint16Array([
    512,
    26465,
    29036,
    7,
    0,
    2,
    4,
    116,
    24638,
    116,
    24636,
    8693,
    29807,
    24610,
    621,
    1,
    0,
    0,
    3,
    112,
    24614,
    111,
    115,
    24615
  ]);

  // node_modules/entities/dist/internal/bin-trie-flags.js
  var BinTrieFlags;
  (function(BinTrieFlags2) {
    BinTrieFlags2[BinTrieFlags2["VALUE_LENGTH"] = 49152] = "VALUE_LENGTH";
    BinTrieFlags2[BinTrieFlags2["FLAG13"] = 8192] = "FLAG13";
    BinTrieFlags2[BinTrieFlags2["BRANCH_LENGTH"] = 8064] = "BRANCH_LENGTH";
    BinTrieFlags2[BinTrieFlags2["JUMP_TABLE"] = 127] = "JUMP_TABLE";
    BinTrieFlags2[BinTrieFlags2["VALUE_MASK"] = 8191] = "VALUE_MASK";
  })(BinTrieFlags || (BinTrieFlags = {}));

  // node_modules/entities/dist/decode.js
  var CharCodes;
  (function(CharCodes2) {
    CharCodes2[CharCodes2["AMP"] = 38] = "AMP";
    CharCodes2[CharCodes2["NUM"] = 35] = "NUM";
    CharCodes2[CharCodes2["SEMI"] = 59] = "SEMI";
    CharCodes2[CharCodes2["EQUALS"] = 61] = "EQUALS";
    CharCodes2[CharCodes2["ZERO"] = 48] = "ZERO";
    CharCodes2[CharCodes2["NINE"] = 57] = "NINE";
    CharCodes2[CharCodes2["LOWER_A"] = 97] = "LOWER_A";
    CharCodes2[CharCodes2["LOWER_X"] = 120] = "LOWER_X";
  })(CharCodes || (CharCodes = {}));
  var TO_LOWER_BIT = 32;
  function isNumber(code) {
    return code - CharCodes.ZERO >>> 0 <= 9;
  }
  function isHexadecimalCharacter(code) {
    return (code | TO_LOWER_BIT) - CharCodes.LOWER_A >>> 0 <= 5;
  }
  function isAlpha(code) {
    return (code | TO_LOWER_BIT) - CharCodes.LOWER_A >>> 0 <= 25;
  }
  function isEntityInAttributeInvalidEnd(code) {
    return code === CharCodes.EQUALS || isAlpha(code) || isNumber(code);
  }
  var EntityDecoderState;
  (function(EntityDecoderState2) {
    EntityDecoderState2[EntityDecoderState2["EntityStart"] = 0] = "EntityStart";
    EntityDecoderState2[EntityDecoderState2["NumericStart"] = 1] = "NumericStart";
    EntityDecoderState2[EntityDecoderState2["NumericDecimal"] = 2] = "NumericDecimal";
    EntityDecoderState2[EntityDecoderState2["NumericHex"] = 3] = "NumericHex";
    EntityDecoderState2[EntityDecoderState2["NamedEntity"] = 4] = "NamedEntity";
  })(EntityDecoderState || (EntityDecoderState = {}));
  var DecodingMode;
  (function(DecodingMode2) {
    DecodingMode2[DecodingMode2["Legacy"] = 0] = "Legacy";
    DecodingMode2[DecodingMode2["Strict"] = 1] = "Strict";
    DecodingMode2[DecodingMode2["Attribute"] = 2] = "Attribute";
  })(DecodingMode || (DecodingMode = {}));
  var EntityDecoder = class {
    constructor(decodeTree, emitCodePoint, errors) {
      __publicField(this, "decodeTree");
      __publicField(this, "emitCodePoint");
      __publicField(this, "errors");
      /** The current state of the decoder. */
      __publicField(this, "state", EntityDecoderState.EntityStart);
      /** Characters that were consumed while parsing an entity. */
      __publicField(this, "consumed", 1);
      /**
       * The result of the entity.
       *
       * For named entities: the trie index of the best legacy match so far
       * (0 = none). For numeric entities: the accumulated code point.
       */
      __publicField(this, "result", 0);
      /** The current index in the decode tree. */
      __publicField(this, "treeIndex", 0);
      /**
       * Characters consumed since the last recorded legacy match, plus one.
       * Invariant at the top of the `stateNamedEntity` loop: `excess` equals
       * the number of unrecorded consumed characters + 1.
       */
      // biome-ignore lint/correctness/noUnusedPrivateClassMembers: False positive (read via destructuring)
      __publicField(this, "excess", 1);
      /** The mode in which the decoder is operating. */
      __publicField(this, "decodeMode", DecodingMode.Strict);
      /** The number of characters that have been consumed in the current run. */
      // biome-ignore lint/correctness/noUnusedPrivateClassMembers: False positive
      __publicField(this, "runConsumed", 0);
      this.decodeTree = decodeTree;
      this.emitCodePoint = emitCodePoint;
      this.errors = errors;
    }
    /**
     * Resets the instance to make it reusable.
     * @param decodeMode Entity decoding mode to use.
     */
    startEntity(decodeMode) {
      this.decodeMode = decodeMode;
      this.state = EntityDecoderState.EntityStart;
      this.result = 0;
      this.treeIndex = 0;
      this.excess = 1;
      this.consumed = 1;
      this.runConsumed = 0;
    }
    /**
     * Write an entity to the decoder. This can be called multiple times with partial entities.
     * If the entity is incomplete, the decoder will return -1.
     *
     * Mirrors the non-streaming `decodeWithTrie`, but with the ability to stop decoding if the
     * entity is incomplete, and resume when the next string is written.
     * @param input The string containing the entity (or a continuation of the entity).
     * @param offset The offset at which the entity begins. Should be 0 if this is not the first call.
     * @returns The number of characters that were consumed, or -1 if the entity is incomplete.
     */
    write(input, offset) {
      switch (this.state) {
        case EntityDecoderState.EntityStart: {
          if (input.charCodeAt(offset) === CharCodes.NUM) {
            this.state = EntityDecoderState.NumericStart;
            this.consumed += 1;
            return this.stateNumericStart(input, offset + 1);
          }
          this.state = EntityDecoderState.NamedEntity;
          return this.stateNamedEntity(input, offset);
        }
        case EntityDecoderState.NumericStart: {
          return this.stateNumericStart(input, offset);
        }
        case EntityDecoderState.NumericDecimal: {
          return this.stateNumericDecimal(input, offset);
        }
        case EntityDecoderState.NumericHex: {
          return this.stateNumericHex(input, offset);
        }
        default: {
          return this.stateNamedEntity(input, offset);
        }
      }
    }
    /**
     * Switches between the numeric decimal and hexadecimal states.
     *
     * Equivalent to the `Numeric character reference state` in the HTML spec.
     * @param input The string containing the entity (or a continuation of the entity).
     * @param offset The current offset.
     * @returns The number of characters that were consumed, or -1 if the entity is incomplete.
     */
    // eslint-disable-next-line unicorn/consistent-class-member-order
    stateNumericStart(input, offset) {
      if (offset >= input.length) {
        return -1;
      }
      if ((input.charCodeAt(offset) | TO_LOWER_BIT) === CharCodes.LOWER_X) {
        this.state = EntityDecoderState.NumericHex;
        this.consumed += 1;
        return this.stateNumericHex(input, offset + 1);
      }
      this.state = EntityDecoderState.NumericDecimal;
      return this.stateNumericDecimal(input, offset);
    }
    /**
     * Parses a hexadecimal numeric entity.
     *
     * Equivalent to the `Hexademical character reference state` in the HTML
     * spec. Digit parsing matches the hex loop in `parseNumericEntity`.
     * The accumulated value is preserved for numeric validation callbacks.
     * @param input The string containing the entity (or a continuation of the entity).
     * @param offset The current offset.
     * @returns The number of characters that were consumed, or -1 if the entity is incomplete.
     */
    stateNumericHex(input, offset) {
      const inputLength = input.length;
      let { result } = this;
      let { consumed } = this;
      while (offset < inputLength) {
        const char = input.charCodeAt(offset);
        if (isNumber(char) || isHexadecimalCharacter(char)) {
          const digit = char <= CharCodes.NINE ? char - CharCodes.ZERO : (char | TO_LOWER_BIT) - CharCodes.LOWER_A + 10;
          result = result * 16 + digit;
          consumed += 1;
          offset += 1;
        } else {
          this.result = result;
          this.consumed = consumed;
          return this.emitNumericEntity(char, 3);
        }
      }
      this.result = result;
      this.consumed = consumed;
      return -1;
    }
    /**
     * Parses a decimal numeric entity.
     *
     * Equivalent to the `Decimal character reference state` in the HTML
     * spec. Digit parsing matches the decimal loop in `parseNumericEntity`.
     * The accumulated value is preserved for numeric validation callbacks.
     * @param input The string containing the entity (or a continuation of the entity).
     * @param offset The current offset.
     * @returns The number of characters that were consumed, or -1 if the entity is incomplete.
     */
    stateNumericDecimal(input, offset) {
      const inputLength = input.length;
      let { result } = this;
      let { consumed } = this;
      while (offset < inputLength) {
        const digit = input.charCodeAt(offset) - CharCodes.ZERO;
        if (digit >>> 0 > 9) {
          this.result = result;
          this.consumed = consumed;
          return this.emitNumericEntity(digit + CharCodes.ZERO, 2);
        }
        result = result * 10 + digit;
        consumed += 1;
        offset += 1;
      }
      this.result = result;
      this.consumed = consumed;
      return -1;
    }
    /**
     * Validate and emit a numeric entity.
     *
     * Implements the logic from the `Hexademical character reference start
     * state` and `Numeric character reference end state` in the HTML spec.
     * @param lastCp The last code point of the entity. Used to see if the
     *               entity was terminated with a semicolon.
     * @param expectedLength The minimum number of characters that should be
     *                       consumed. Used to validate that at least one digit
     *                       was consumed.
     * @returns The number of characters that were consumed.
     */
    emitNumericEntity(lastCp, expectedLength) {
      if (this.consumed <= expectedLength) {
        this.errors?.absenceOfDigitsInNumericCharacterReference(this.consumed);
        return 0;
      }
      if (lastCp === CharCodes.SEMI) {
        this.consumed += 1;
      } else if (this.decodeMode === DecodingMode.Strict) {
        return 0;
      }
      this.emitCodePoint((this.decodeTree === xmlDecodeTree ? replaceCodePointXML : replaceCodePoint)(this.result), this.consumed);
      if (this.errors) {
        if (lastCp !== CharCodes.SEMI) {
          this.errors.missingSemicolonAfterCharacterReference();
        }
        this.errors.validateNumericCharacterReference(this.result);
      }
      return this.consumed;
    }
    /**
     * Flush locally-tracked walk state back to the fields, then emit the
     * recorded legacy match or reject (cold path — at most once per
     * entity). Called after failed navigation (leaf node, branch miss, or
     * compact-run mismatch). In attribute mode, reject if no legacy was
     * recorded at the current node, if we descended past it, or if the
     * pending input character is an invalid attribute terminator.
     * @param consumed Locally-tracked consumed count.
     * @param excess Locally-tracked excess count.
     * @param char Pending input character (may be the mismatching char).
     * @param valueLength Value length at the current trie node.
     */
    flushAndEmitLegacyOrReject(consumed, excess, char, valueLength) {
      this.consumed = consumed;
      this.excess = excess;
      return this.result === 0 || this.decodeMode === DecodingMode.Attribute && (valueLength === 0 || excess > 1 || isEntityInAttributeInvalidEnd(char)) ? 0 : this.emitNotTerminatedNamedEntity();
    }
    /**
     * Parses a named entity.
     *
     * Equivalent to the `Named character reference state` in the HTML spec.
     * @param input The string containing the entity (or a continuation of the entity).
     * @param offset The current offset.
     * @returns The number of characters that were consumed, or -1 if the entity is incomplete.
     */
    stateNamedEntity(input, offset) {
      const { decodeTree } = this;
      const inputLength = input.length;
      const isStrict = this.decodeMode === DecodingMode.Strict;
      let { treeIndex } = this;
      let { excess } = this;
      let { consumed } = this;
      let current = decodeTree[treeIndex];
      while (offset < inputLength) {
        while ((current & (BinTrieFlags.VALUE_LENGTH | BinTrieFlags.FLAG13)) === 0 && (current & BinTrieFlags.JUMP_TABLE) !== 0) {
          const char2 = input.charCodeAt(offset);
          const jumpOffset = current & BinTrieFlags.JUMP_TABLE;
          const branchCount = (current & BinTrieFlags.BRANCH_LENGTH) >> 7;
          if (branchCount === 0) {
            if (char2 !== jumpOffset) {
              return this.flushAndEmitLegacyOrReject(consumed, excess, char2, 0);
            }
            treeIndex += 1;
          } else {
            const slot = char2 - jumpOffset;
            if (slot >>> 0 >= branchCount) {
              return this.flushAndEmitLegacyOrReject(consumed, excess, char2, 0);
            }
            const stored = decodeTree[treeIndex + 1 + slot];
            if (stored === 0) {
              return this.flushAndEmitLegacyOrReject(consumed, excess, char2, 0);
            }
            treeIndex = treeIndex + branchCount + stored & 65535;
          }
          current = decodeTree[treeIndex];
          offset += 1;
          excess += 1;
          if (offset >= inputLength)
            break;
        }
        if (offset >= inputLength)
          break;
        if ((current & (BinTrieFlags.VALUE_LENGTH | BinTrieFlags.FLAG13)) === BinTrieFlags.FLAG13) {
          const runLength = (current & BinTrieFlags.BRANCH_LENGTH) >> 7;
          let { runConsumed } = this;
          if (runConsumed === 0) {
            const char2 = input.charCodeAt(offset);
            if (char2 !== (current & BinTrieFlags.JUMP_TABLE)) {
              return this.flushAndEmitLegacyOrReject(consumed, excess, char2, 0);
            }
            offset += 1;
            excess += 1;
            runConsumed = 1;
          }
          while (runConsumed < runLength) {
            if (offset >= inputLength) {
              this.treeIndex = treeIndex;
              this.excess = excess;
              this.consumed = consumed;
              this.runConsumed = runConsumed;
              return -1;
            }
            const charIndexInPacked = runConsumed - 1;
            const packedWord = decodeTree[treeIndex + 1 + (charIndexInPacked >> 1)];
            const expectedChar = packedWord >> ((charIndexInPacked & 1) << 3) & 255;
            const char2 = input.charCodeAt(offset);
            if (char2 !== expectedChar) {
              this.runConsumed = 0;
              return this.flushAndEmitLegacyOrReject(consumed, excess, char2, 0);
            }
            offset += 1;
            excess += 1;
            runConsumed += 1;
          }
          this.runConsumed = 0;
          treeIndex += 1 + (runLength >> 1);
          current = decodeTree[treeIndex];
          continue;
        }
        const valueLength = current >>> 14;
        const char = input.charCodeAt(offset);
        if (valueLength !== 0) {
          if (!isStrict && (current & BinTrieFlags.FLAG13) === 0) {
            this.result = treeIndex;
            consumed += excess - 1;
            excess = 1;
          }
          if (char === CharCodes.SEMI) {
            return this.emitNamedEntityData(treeIndex, valueLength, consumed + excess);
          }
          if (valueLength === 1) {
            return this.flushAndEmitLegacyOrReject(consumed, excess, char, valueLength);
          }
        }
        const next = determineBranch(decodeTree, current, treeIndex + (valueLength || 1), char);
        if (next < 0) {
          return this.flushAndEmitLegacyOrReject(consumed, excess, char, valueLength);
        }
        treeIndex = next;
        current = decodeTree[treeIndex];
        offset += 1;
        excess += 1;
      }
      if (!isStrict && current >>> 14 !== 0 && (current & BinTrieFlags.FLAG13) === 0) {
        this.result = treeIndex;
        consumed += excess - 1;
        excess = 1;
      }
      this.treeIndex = treeIndex;
      this.excess = excess;
      this.consumed = consumed;
      return -1;
    }
    /**
     * Emit a named entity that was not terminated with a semicolon.
     * @returns The number of characters consumed.
     */
    emitNotTerminatedNamedEntity() {
      const { result, decodeTree } = this;
      const valueLength = decodeTree[result] >>> 14;
      this.emitNamedEntityData(result, valueLength, this.consumed);
      this.errors?.missingSemicolonAfterCharacterReference();
      return this.consumed;
    }
    /**
     * Emit a named entity.
     * @param result The index of the entity in the decode tree.
     * @param valueLength Encoded value length (header plus any value words).
     * @param consumed The number of characters consumed.
     * @returns The number of characters consumed.
     */
    emitNamedEntityData(result, valueLength, consumed) {
      const { decodeTree } = this;
      this.emitCodePoint(valueLength === 1 ? decodeTree[result] & BinTrieFlags.VALUE_MASK : decodeTree[result + 1], consumed);
      if (valueLength === 3) {
        this.emitCodePoint(decodeTree[result + 2], consumed);
      }
      return consumed;
    }
    /**
     * Signal to the parser that the end of the input was reached.
     *
     * Remaining data will be emitted and relevant errors will be produced.
     * @returns The number of characters consumed.
     */
    end() {
      switch (this.state) {
        case EntityDecoderState.NamedEntity: {
          return this.result !== 0 && (this.decodeMode !== DecodingMode.Attribute || this.result === this.treeIndex) ? this.emitNotTerminatedNamedEntity() : 0;
        }
        // Otherwise, emit a numeric entity if we have one.
        case EntityDecoderState.NumericDecimal: {
          return this.emitNumericEntity(0, 2);
        }
        case EntityDecoderState.NumericHex: {
          return this.emitNumericEntity(0, 3);
        }
        case EntityDecoderState.NumericStart: {
          this.errors?.absenceOfDigitsInNumericCharacterReference(this.consumed);
          return 0;
        }
        default: {
          return 0;
        }
      }
    }
  };
  function determineBranch(decodeTree, current, nodeIndex, char) {
    const branchCount = (current & BinTrieFlags.BRANCH_LENGTH) >> 7;
    const jumpOffset = current & BinTrieFlags.JUMP_TABLE;
    if (jumpOffset) {
      if (branchCount === 0) {
        return char === jumpOffset ? nodeIndex : -1;
      }
      const slot = char - jumpOffset;
      if (slot >>> 0 >= branchCount)
        return -1;
      const stored = decodeTree[nodeIndex + slot];
      return stored === 0 ? -1 : nodeIndex + branchCount + stored - 1 & 65535;
    }
    if (branchCount === 0)
      return -1;
    const packedKeySlots = branchCount + 1 >> 1;
    const branchEnd = nodeIndex + packedKeySlots + branchCount;
    for (let index = 0; index < branchCount; index++) {
      const packed = decodeTree[nodeIndex + (index >> 1)];
      const key = packed >> ((index & 1) << 3) & 255;
      if (key === char) {
        const pointerIndex = nodeIndex + packedKeySlots + index;
        return branchEnd + decodeTree[pointerIndex] & 65535;
      }
      if (key > char)
        return -1;
    }
    return -1;
  }

  // src/common/html.ts
  var html_exports = {};
  __export(html_exports, {
    ATTRS: () => ATTRS,
    DOCUMENT_MODE: () => DOCUMENT_MODE,
    NS: () => NS,
    NUMBERED_HEADERS: () => NUMBERED_HEADERS,
    SPECIAL_ELEMENTS: () => SPECIAL_ELEMENTS,
    TAG_ID: () => TAG_ID,
    TAG_NAMES: () => TAG_NAMES,
    getTagID: () => getTagID,
    hasUnescapedText: () => hasUnescapedText
  });
  var NS = /* @__PURE__ */ ((NS2) => {
    NS2["HTML"] = "http://www.w3.org/1999/xhtml";
    NS2["MATHML"] = "http://www.w3.org/1998/Math/MathML";
    NS2["SVG"] = "http://www.w3.org/2000/svg";
    NS2["XLINK"] = "http://www.w3.org/1999/xlink";
    NS2["XML"] = "http://www.w3.org/XML/1998/namespace";
    NS2["XMLNS"] = "http://www.w3.org/2000/xmlns/";
    return NS2;
  })(NS || {});
  var ATTRS = /* @__PURE__ */ ((ATTRS2) => {
    ATTRS2["TYPE"] = "type";
    ATTRS2["ACTION"] = "action";
    ATTRS2["ENCODING"] = "encoding";
    ATTRS2["PROMPT"] = "prompt";
    ATTRS2["NAME"] = "name";
    ATTRS2["COLOR"] = "color";
    ATTRS2["FACE"] = "face";
    ATTRS2["SIZE"] = "size";
    return ATTRS2;
  })(ATTRS || {});
  var DOCUMENT_MODE = /* @__PURE__ */ ((DOCUMENT_MODE2) => {
    DOCUMENT_MODE2["NO_QUIRKS"] = "no-quirks";
    DOCUMENT_MODE2["QUIRKS"] = "quirks";
    DOCUMENT_MODE2["LIMITED_QUIRKS"] = "limited-quirks";
    return DOCUMENT_MODE2;
  })(DOCUMENT_MODE || {});
  var TAG_NAMES = /* @__PURE__ */ ((TAG_NAMES2) => {
    TAG_NAMES2["A"] = "a";
    TAG_NAMES2["ADDRESS"] = "address";
    TAG_NAMES2["ANNOTATION_XML"] = "annotation-xml";
    TAG_NAMES2["APPLET"] = "applet";
    TAG_NAMES2["AREA"] = "area";
    TAG_NAMES2["ARTICLE"] = "article";
    TAG_NAMES2["ASIDE"] = "aside";
    TAG_NAMES2["B"] = "b";
    TAG_NAMES2["BASE"] = "base";
    TAG_NAMES2["BASEFONT"] = "basefont";
    TAG_NAMES2["BGSOUND"] = "bgsound";
    TAG_NAMES2["BIG"] = "big";
    TAG_NAMES2["BLOCKQUOTE"] = "blockquote";
    TAG_NAMES2["BODY"] = "body";
    TAG_NAMES2["BR"] = "br";
    TAG_NAMES2["BUTTON"] = "button";
    TAG_NAMES2["CAPTION"] = "caption";
    TAG_NAMES2["CENTER"] = "center";
    TAG_NAMES2["CODE"] = "code";
    TAG_NAMES2["COL"] = "col";
    TAG_NAMES2["COLGROUP"] = "colgroup";
    TAG_NAMES2["DD"] = "dd";
    TAG_NAMES2["DESC"] = "desc";
    TAG_NAMES2["DETAILS"] = "details";
    TAG_NAMES2["DIALOG"] = "dialog";
    TAG_NAMES2["DIR"] = "dir";
    TAG_NAMES2["DIV"] = "div";
    TAG_NAMES2["DL"] = "dl";
    TAG_NAMES2["DT"] = "dt";
    TAG_NAMES2["EM"] = "em";
    TAG_NAMES2["EMBED"] = "embed";
    TAG_NAMES2["FIELDSET"] = "fieldset";
    TAG_NAMES2["FIGCAPTION"] = "figcaption";
    TAG_NAMES2["FIGURE"] = "figure";
    TAG_NAMES2["FONT"] = "font";
    TAG_NAMES2["FOOTER"] = "footer";
    TAG_NAMES2["FOREIGN_OBJECT"] = "foreignObject";
    TAG_NAMES2["FORM"] = "form";
    TAG_NAMES2["FRAME"] = "frame";
    TAG_NAMES2["FRAMESET"] = "frameset";
    TAG_NAMES2["H1"] = "h1";
    TAG_NAMES2["H2"] = "h2";
    TAG_NAMES2["H3"] = "h3";
    TAG_NAMES2["H4"] = "h4";
    TAG_NAMES2["H5"] = "h5";
    TAG_NAMES2["H6"] = "h6";
    TAG_NAMES2["HEAD"] = "head";
    TAG_NAMES2["HEADER"] = "header";
    TAG_NAMES2["HGROUP"] = "hgroup";
    TAG_NAMES2["HR"] = "hr";
    TAG_NAMES2["HTML"] = "html";
    TAG_NAMES2["I"] = "i";
    TAG_NAMES2["IMG"] = "img";
    TAG_NAMES2["IMAGE"] = "image";
    TAG_NAMES2["INPUT"] = "input";
    TAG_NAMES2["IFRAME"] = "iframe";
    TAG_NAMES2["KEYGEN"] = "keygen";
    TAG_NAMES2["LABEL"] = "label";
    TAG_NAMES2["LI"] = "li";
    TAG_NAMES2["LINK"] = "link";
    TAG_NAMES2["LISTING"] = "listing";
    TAG_NAMES2["MAIN"] = "main";
    TAG_NAMES2["MALIGNMARK"] = "malignmark";
    TAG_NAMES2["MARQUEE"] = "marquee";
    TAG_NAMES2["MATH"] = "math";
    TAG_NAMES2["MENU"] = "menu";
    TAG_NAMES2["META"] = "meta";
    TAG_NAMES2["MGLYPH"] = "mglyph";
    TAG_NAMES2["MI"] = "mi";
    TAG_NAMES2["MO"] = "mo";
    TAG_NAMES2["MN"] = "mn";
    TAG_NAMES2["MS"] = "ms";
    TAG_NAMES2["MTEXT"] = "mtext";
    TAG_NAMES2["NAV"] = "nav";
    TAG_NAMES2["NOBR"] = "nobr";
    TAG_NAMES2["NOFRAMES"] = "noframes";
    TAG_NAMES2["NOEMBED"] = "noembed";
    TAG_NAMES2["NOSCRIPT"] = "noscript";
    TAG_NAMES2["OBJECT"] = "object";
    TAG_NAMES2["OL"] = "ol";
    TAG_NAMES2["OPTGROUP"] = "optgroup";
    TAG_NAMES2["OPTION"] = "option";
    TAG_NAMES2["P"] = "p";
    TAG_NAMES2["PARAM"] = "param";
    TAG_NAMES2["PLAINTEXT"] = "plaintext";
    TAG_NAMES2["PRE"] = "pre";
    TAG_NAMES2["RB"] = "rb";
    TAG_NAMES2["RP"] = "rp";
    TAG_NAMES2["RT"] = "rt";
    TAG_NAMES2["RTC"] = "rtc";
    TAG_NAMES2["RUBY"] = "ruby";
    TAG_NAMES2["S"] = "s";
    TAG_NAMES2["SCRIPT"] = "script";
    TAG_NAMES2["SEARCH"] = "search";
    TAG_NAMES2["SECTION"] = "section";
    TAG_NAMES2["SELECT"] = "select";
    TAG_NAMES2["SOURCE"] = "source";
    TAG_NAMES2["SMALL"] = "small";
    TAG_NAMES2["SPAN"] = "span";
    TAG_NAMES2["STRIKE"] = "strike";
    TAG_NAMES2["STRONG"] = "strong";
    TAG_NAMES2["STYLE"] = "style";
    TAG_NAMES2["SUB"] = "sub";
    TAG_NAMES2["SUMMARY"] = "summary";
    TAG_NAMES2["SUP"] = "sup";
    TAG_NAMES2["TABLE"] = "table";
    TAG_NAMES2["TBODY"] = "tbody";
    TAG_NAMES2["TEMPLATE"] = "template";
    TAG_NAMES2["TEXTAREA"] = "textarea";
    TAG_NAMES2["TFOOT"] = "tfoot";
    TAG_NAMES2["TD"] = "td";
    TAG_NAMES2["TH"] = "th";
    TAG_NAMES2["THEAD"] = "thead";
    TAG_NAMES2["TITLE"] = "title";
    TAG_NAMES2["TR"] = "tr";
    TAG_NAMES2["TRACK"] = "track";
    TAG_NAMES2["TT"] = "tt";
    TAG_NAMES2["U"] = "u";
    TAG_NAMES2["UL"] = "ul";
    TAG_NAMES2["SVG"] = "svg";
    TAG_NAMES2["VAR"] = "var";
    TAG_NAMES2["WBR"] = "wbr";
    TAG_NAMES2["XMP"] = "xmp";
    return TAG_NAMES2;
  })(TAG_NAMES || {});
  var TAG_ID = /* @__PURE__ */ ((TAG_ID2) => {
    TAG_ID2[TAG_ID2["UNKNOWN"] = 0] = "UNKNOWN";
    TAG_ID2[TAG_ID2["A"] = 1] = "A";
    TAG_ID2[TAG_ID2["ADDRESS"] = 2] = "ADDRESS";
    TAG_ID2[TAG_ID2["ANNOTATION_XML"] = 3] = "ANNOTATION_XML";
    TAG_ID2[TAG_ID2["APPLET"] = 4] = "APPLET";
    TAG_ID2[TAG_ID2["AREA"] = 5] = "AREA";
    TAG_ID2[TAG_ID2["ARTICLE"] = 6] = "ARTICLE";
    TAG_ID2[TAG_ID2["ASIDE"] = 7] = "ASIDE";
    TAG_ID2[TAG_ID2["B"] = 8] = "B";
    TAG_ID2[TAG_ID2["BASE"] = 9] = "BASE";
    TAG_ID2[TAG_ID2["BASEFONT"] = 10] = "BASEFONT";
    TAG_ID2[TAG_ID2["BGSOUND"] = 11] = "BGSOUND";
    TAG_ID2[TAG_ID2["BIG"] = 12] = "BIG";
    TAG_ID2[TAG_ID2["BLOCKQUOTE"] = 13] = "BLOCKQUOTE";
    TAG_ID2[TAG_ID2["BODY"] = 14] = "BODY";
    TAG_ID2[TAG_ID2["BR"] = 15] = "BR";
    TAG_ID2[TAG_ID2["BUTTON"] = 16] = "BUTTON";
    TAG_ID2[TAG_ID2["CAPTION"] = 17] = "CAPTION";
    TAG_ID2[TAG_ID2["CENTER"] = 18] = "CENTER";
    TAG_ID2[TAG_ID2["CODE"] = 19] = "CODE";
    TAG_ID2[TAG_ID2["COL"] = 20] = "COL";
    TAG_ID2[TAG_ID2["COLGROUP"] = 21] = "COLGROUP";
    TAG_ID2[TAG_ID2["DD"] = 22] = "DD";
    TAG_ID2[TAG_ID2["DESC"] = 23] = "DESC";
    TAG_ID2[TAG_ID2["DETAILS"] = 24] = "DETAILS";
    TAG_ID2[TAG_ID2["DIALOG"] = 25] = "DIALOG";
    TAG_ID2[TAG_ID2["DIR"] = 26] = "DIR";
    TAG_ID2[TAG_ID2["DIV"] = 27] = "DIV";
    TAG_ID2[TAG_ID2["DL"] = 28] = "DL";
    TAG_ID2[TAG_ID2["DT"] = 29] = "DT";
    TAG_ID2[TAG_ID2["EM"] = 30] = "EM";
    TAG_ID2[TAG_ID2["EMBED"] = 31] = "EMBED";
    TAG_ID2[TAG_ID2["FIELDSET"] = 32] = "FIELDSET";
    TAG_ID2[TAG_ID2["FIGCAPTION"] = 33] = "FIGCAPTION";
    TAG_ID2[TAG_ID2["FIGURE"] = 34] = "FIGURE";
    TAG_ID2[TAG_ID2["FONT"] = 35] = "FONT";
    TAG_ID2[TAG_ID2["FOOTER"] = 36] = "FOOTER";
    TAG_ID2[TAG_ID2["FOREIGN_OBJECT"] = 37] = "FOREIGN_OBJECT";
    TAG_ID2[TAG_ID2["FORM"] = 38] = "FORM";
    TAG_ID2[TAG_ID2["FRAME"] = 39] = "FRAME";
    TAG_ID2[TAG_ID2["FRAMESET"] = 40] = "FRAMESET";
    TAG_ID2[TAG_ID2["H1"] = 41] = "H1";
    TAG_ID2[TAG_ID2["H2"] = 42] = "H2";
    TAG_ID2[TAG_ID2["H3"] = 43] = "H3";
    TAG_ID2[TAG_ID2["H4"] = 44] = "H4";
    TAG_ID2[TAG_ID2["H5"] = 45] = "H5";
    TAG_ID2[TAG_ID2["H6"] = 46] = "H6";
    TAG_ID2[TAG_ID2["HEAD"] = 47] = "HEAD";
    TAG_ID2[TAG_ID2["HEADER"] = 48] = "HEADER";
    TAG_ID2[TAG_ID2["HGROUP"] = 49] = "HGROUP";
    TAG_ID2[TAG_ID2["HR"] = 50] = "HR";
    TAG_ID2[TAG_ID2["HTML"] = 51] = "HTML";
    TAG_ID2[TAG_ID2["I"] = 52] = "I";
    TAG_ID2[TAG_ID2["IMG"] = 53] = "IMG";
    TAG_ID2[TAG_ID2["IMAGE"] = 54] = "IMAGE";
    TAG_ID2[TAG_ID2["INPUT"] = 55] = "INPUT";
    TAG_ID2[TAG_ID2["IFRAME"] = 56] = "IFRAME";
    TAG_ID2[TAG_ID2["KEYGEN"] = 57] = "KEYGEN";
    TAG_ID2[TAG_ID2["LABEL"] = 58] = "LABEL";
    TAG_ID2[TAG_ID2["LI"] = 59] = "LI";
    TAG_ID2[TAG_ID2["LINK"] = 60] = "LINK";
    TAG_ID2[TAG_ID2["LISTING"] = 61] = "LISTING";
    TAG_ID2[TAG_ID2["MAIN"] = 62] = "MAIN";
    TAG_ID2[TAG_ID2["MALIGNMARK"] = 63] = "MALIGNMARK";
    TAG_ID2[TAG_ID2["MARQUEE"] = 64] = "MARQUEE";
    TAG_ID2[TAG_ID2["MATH"] = 65] = "MATH";
    TAG_ID2[TAG_ID2["MENU"] = 66] = "MENU";
    TAG_ID2[TAG_ID2["META"] = 67] = "META";
    TAG_ID2[TAG_ID2["MGLYPH"] = 68] = "MGLYPH";
    TAG_ID2[TAG_ID2["MI"] = 69] = "MI";
    TAG_ID2[TAG_ID2["MO"] = 70] = "MO";
    TAG_ID2[TAG_ID2["MN"] = 71] = "MN";
    TAG_ID2[TAG_ID2["MS"] = 72] = "MS";
    TAG_ID2[TAG_ID2["MTEXT"] = 73] = "MTEXT";
    TAG_ID2[TAG_ID2["NAV"] = 74] = "NAV";
    TAG_ID2[TAG_ID2["NOBR"] = 75] = "NOBR";
    TAG_ID2[TAG_ID2["NOFRAMES"] = 76] = "NOFRAMES";
    TAG_ID2[TAG_ID2["NOEMBED"] = 77] = "NOEMBED";
    TAG_ID2[TAG_ID2["NOSCRIPT"] = 78] = "NOSCRIPT";
    TAG_ID2[TAG_ID2["OBJECT"] = 79] = "OBJECT";
    TAG_ID2[TAG_ID2["OL"] = 80] = "OL";
    TAG_ID2[TAG_ID2["OPTGROUP"] = 81] = "OPTGROUP";
    TAG_ID2[TAG_ID2["OPTION"] = 82] = "OPTION";
    TAG_ID2[TAG_ID2["P"] = 83] = "P";
    TAG_ID2[TAG_ID2["PARAM"] = 84] = "PARAM";
    TAG_ID2[TAG_ID2["PLAINTEXT"] = 85] = "PLAINTEXT";
    TAG_ID2[TAG_ID2["PRE"] = 86] = "PRE";
    TAG_ID2[TAG_ID2["RB"] = 87] = "RB";
    TAG_ID2[TAG_ID2["RP"] = 88] = "RP";
    TAG_ID2[TAG_ID2["RT"] = 89] = "RT";
    TAG_ID2[TAG_ID2["RTC"] = 90] = "RTC";
    TAG_ID2[TAG_ID2["RUBY"] = 91] = "RUBY";
    TAG_ID2[TAG_ID2["S"] = 92] = "S";
    TAG_ID2[TAG_ID2["SCRIPT"] = 93] = "SCRIPT";
    TAG_ID2[TAG_ID2["SEARCH"] = 94] = "SEARCH";
    TAG_ID2[TAG_ID2["SECTION"] = 95] = "SECTION";
    TAG_ID2[TAG_ID2["SELECT"] = 96] = "SELECT";
    TAG_ID2[TAG_ID2["SOURCE"] = 97] = "SOURCE";
    TAG_ID2[TAG_ID2["SMALL"] = 98] = "SMALL";
    TAG_ID2[TAG_ID2["SPAN"] = 99] = "SPAN";
    TAG_ID2[TAG_ID2["STRIKE"] = 100] = "STRIKE";
    TAG_ID2[TAG_ID2["STRONG"] = 101] = "STRONG";
    TAG_ID2[TAG_ID2["STYLE"] = 102] = "STYLE";
    TAG_ID2[TAG_ID2["SUB"] = 103] = "SUB";
    TAG_ID2[TAG_ID2["SUMMARY"] = 104] = "SUMMARY";
    TAG_ID2[TAG_ID2["SUP"] = 105] = "SUP";
    TAG_ID2[TAG_ID2["TABLE"] = 106] = "TABLE";
    TAG_ID2[TAG_ID2["TBODY"] = 107] = "TBODY";
    TAG_ID2[TAG_ID2["TEMPLATE"] = 108] = "TEMPLATE";
    TAG_ID2[TAG_ID2["TEXTAREA"] = 109] = "TEXTAREA";
    TAG_ID2[TAG_ID2["TFOOT"] = 110] = "TFOOT";
    TAG_ID2[TAG_ID2["TD"] = 111] = "TD";
    TAG_ID2[TAG_ID2["TH"] = 112] = "TH";
    TAG_ID2[TAG_ID2["THEAD"] = 113] = "THEAD";
    TAG_ID2[TAG_ID2["TITLE"] = 114] = "TITLE";
    TAG_ID2[TAG_ID2["TR"] = 115] = "TR";
    TAG_ID2[TAG_ID2["TRACK"] = 116] = "TRACK";
    TAG_ID2[TAG_ID2["TT"] = 117] = "TT";
    TAG_ID2[TAG_ID2["U"] = 118] = "U";
    TAG_ID2[TAG_ID2["UL"] = 119] = "UL";
    TAG_ID2[TAG_ID2["SVG"] = 120] = "SVG";
    TAG_ID2[TAG_ID2["VAR"] = 121] = "VAR";
    TAG_ID2[TAG_ID2["WBR"] = 122] = "WBR";
    TAG_ID2[TAG_ID2["XMP"] = 123] = "XMP";
    return TAG_ID2;
  })(TAG_ID || {});
  var TAG_NAME_TO_ID = /* @__PURE__ */ new Map([
    ["a" /* A */, 1 /* A */],
    ["address" /* ADDRESS */, 2 /* ADDRESS */],
    ["annotation-xml" /* ANNOTATION_XML */, 3 /* ANNOTATION_XML */],
    ["applet" /* APPLET */, 4 /* APPLET */],
    ["area" /* AREA */, 5 /* AREA */],
    ["article" /* ARTICLE */, 6 /* ARTICLE */],
    ["aside" /* ASIDE */, 7 /* ASIDE */],
    ["b" /* B */, 8 /* B */],
    ["base" /* BASE */, 9 /* BASE */],
    ["basefont" /* BASEFONT */, 10 /* BASEFONT */],
    ["bgsound" /* BGSOUND */, 11 /* BGSOUND */],
    ["big" /* BIG */, 12 /* BIG */],
    ["blockquote" /* BLOCKQUOTE */, 13 /* BLOCKQUOTE */],
    ["body" /* BODY */, 14 /* BODY */],
    ["br" /* BR */, 15 /* BR */],
    ["button" /* BUTTON */, 16 /* BUTTON */],
    ["caption" /* CAPTION */, 17 /* CAPTION */],
    ["center" /* CENTER */, 18 /* CENTER */],
    ["code" /* CODE */, 19 /* CODE */],
    ["col" /* COL */, 20 /* COL */],
    ["colgroup" /* COLGROUP */, 21 /* COLGROUP */],
    ["dd" /* DD */, 22 /* DD */],
    ["desc" /* DESC */, 23 /* DESC */],
    ["details" /* DETAILS */, 24 /* DETAILS */],
    ["dialog" /* DIALOG */, 25 /* DIALOG */],
    ["dir" /* DIR */, 26 /* DIR */],
    ["div" /* DIV */, 27 /* DIV */],
    ["dl" /* DL */, 28 /* DL */],
    ["dt" /* DT */, 29 /* DT */],
    ["em" /* EM */, 30 /* EM */],
    ["embed" /* EMBED */, 31 /* EMBED */],
    ["fieldset" /* FIELDSET */, 32 /* FIELDSET */],
    ["figcaption" /* FIGCAPTION */, 33 /* FIGCAPTION */],
    ["figure" /* FIGURE */, 34 /* FIGURE */],
    ["font" /* FONT */, 35 /* FONT */],
    ["footer" /* FOOTER */, 36 /* FOOTER */],
    ["foreignObject" /* FOREIGN_OBJECT */, 37 /* FOREIGN_OBJECT */],
    ["form" /* FORM */, 38 /* FORM */],
    ["frame" /* FRAME */, 39 /* FRAME */],
    ["frameset" /* FRAMESET */, 40 /* FRAMESET */],
    ["h1" /* H1 */, 41 /* H1 */],
    ["h2" /* H2 */, 42 /* H2 */],
    ["h3" /* H3 */, 43 /* H3 */],
    ["h4" /* H4 */, 44 /* H4 */],
    ["h5" /* H5 */, 45 /* H5 */],
    ["h6" /* H6 */, 46 /* H6 */],
    ["head" /* HEAD */, 47 /* HEAD */],
    ["header" /* HEADER */, 48 /* HEADER */],
    ["hgroup" /* HGROUP */, 49 /* HGROUP */],
    ["hr" /* HR */, 50 /* HR */],
    ["html" /* HTML */, 51 /* HTML */],
    ["i" /* I */, 52 /* I */],
    ["img" /* IMG */, 53 /* IMG */],
    ["image" /* IMAGE */, 54 /* IMAGE */],
    ["input" /* INPUT */, 55 /* INPUT */],
    ["iframe" /* IFRAME */, 56 /* IFRAME */],
    ["keygen" /* KEYGEN */, 57 /* KEYGEN */],
    ["label" /* LABEL */, 58 /* LABEL */],
    ["li" /* LI */, 59 /* LI */],
    ["link" /* LINK */, 60 /* LINK */],
    ["listing" /* LISTING */, 61 /* LISTING */],
    ["main" /* MAIN */, 62 /* MAIN */],
    ["malignmark" /* MALIGNMARK */, 63 /* MALIGNMARK */],
    ["marquee" /* MARQUEE */, 64 /* MARQUEE */],
    ["math" /* MATH */, 65 /* MATH */],
    ["menu" /* MENU */, 66 /* MENU */],
    ["meta" /* META */, 67 /* META */],
    ["mglyph" /* MGLYPH */, 68 /* MGLYPH */],
    ["mi" /* MI */, 69 /* MI */],
    ["mo" /* MO */, 70 /* MO */],
    ["mn" /* MN */, 71 /* MN */],
    ["ms" /* MS */, 72 /* MS */],
    ["mtext" /* MTEXT */, 73 /* MTEXT */],
    ["nav" /* NAV */, 74 /* NAV */],
    ["nobr" /* NOBR */, 75 /* NOBR */],
    ["noframes" /* NOFRAMES */, 76 /* NOFRAMES */],
    ["noembed" /* NOEMBED */, 77 /* NOEMBED */],
    ["noscript" /* NOSCRIPT */, 78 /* NOSCRIPT */],
    ["object" /* OBJECT */, 79 /* OBJECT */],
    ["ol" /* OL */, 80 /* OL */],
    ["optgroup" /* OPTGROUP */, 81 /* OPTGROUP */],
    ["option" /* OPTION */, 82 /* OPTION */],
    ["p" /* P */, 83 /* P */],
    ["param" /* PARAM */, 84 /* PARAM */],
    ["plaintext" /* PLAINTEXT */, 85 /* PLAINTEXT */],
    ["pre" /* PRE */, 86 /* PRE */],
    ["rb" /* RB */, 87 /* RB */],
    ["rp" /* RP */, 88 /* RP */],
    ["rt" /* RT */, 89 /* RT */],
    ["rtc" /* RTC */, 90 /* RTC */],
    ["ruby" /* RUBY */, 91 /* RUBY */],
    ["s" /* S */, 92 /* S */],
    ["script" /* SCRIPT */, 93 /* SCRIPT */],
    ["search" /* SEARCH */, 94 /* SEARCH */],
    ["section" /* SECTION */, 95 /* SECTION */],
    ["select" /* SELECT */, 96 /* SELECT */],
    ["source" /* SOURCE */, 97 /* SOURCE */],
    ["small" /* SMALL */, 98 /* SMALL */],
    ["span" /* SPAN */, 99 /* SPAN */],
    ["strike" /* STRIKE */, 100 /* STRIKE */],
    ["strong" /* STRONG */, 101 /* STRONG */],
    ["style" /* STYLE */, 102 /* STYLE */],
    ["sub" /* SUB */, 103 /* SUB */],
    ["summary" /* SUMMARY */, 104 /* SUMMARY */],
    ["sup" /* SUP */, 105 /* SUP */],
    ["table" /* TABLE */, 106 /* TABLE */],
    ["tbody" /* TBODY */, 107 /* TBODY */],
    ["template" /* TEMPLATE */, 108 /* TEMPLATE */],
    ["textarea" /* TEXTAREA */, 109 /* TEXTAREA */],
    ["tfoot" /* TFOOT */, 110 /* TFOOT */],
    ["td" /* TD */, 111 /* TD */],
    ["th" /* TH */, 112 /* TH */],
    ["thead" /* THEAD */, 113 /* THEAD */],
    ["title" /* TITLE */, 114 /* TITLE */],
    ["tr" /* TR */, 115 /* TR */],
    ["track" /* TRACK */, 116 /* TRACK */],
    ["tt" /* TT */, 117 /* TT */],
    ["u" /* U */, 118 /* U */],
    ["ul" /* UL */, 119 /* UL */],
    ["svg" /* SVG */, 120 /* SVG */],
    ["var" /* VAR */, 121 /* VAR */],
    ["wbr" /* WBR */, 122 /* WBR */],
    ["xmp" /* XMP */, 123 /* XMP */]
  ]);
  function getTagID(tagName) {
    return TAG_NAME_TO_ID.get(tagName) ?? 0 /* UNKNOWN */;
  }
  var $ = TAG_ID;
  var SPECIAL_ELEMENTS = {
    ["http://www.w3.org/1999/xhtml" /* HTML */]: /* @__PURE__ */ new Set([
      $.ADDRESS,
      $.APPLET,
      $.AREA,
      $.ARTICLE,
      $.ASIDE,
      $.BASE,
      $.BASEFONT,
      $.BGSOUND,
      $.BLOCKQUOTE,
      $.BODY,
      $.BR,
      $.BUTTON,
      $.CAPTION,
      $.CENTER,
      $.COL,
      $.COLGROUP,
      $.DD,
      $.DETAILS,
      $.DIR,
      $.DIV,
      $.DL,
      $.DT,
      $.EMBED,
      $.FIELDSET,
      $.FIGCAPTION,
      $.FIGURE,
      $.FOOTER,
      $.FORM,
      $.FRAME,
      $.FRAMESET,
      $.H1,
      $.H2,
      $.H3,
      $.H4,
      $.H5,
      $.H6,
      $.HEAD,
      $.HEADER,
      $.HGROUP,
      $.HR,
      $.HTML,
      $.IFRAME,
      $.IMG,
      $.INPUT,
      $.LI,
      $.LINK,
      $.LISTING,
      $.MAIN,
      $.MARQUEE,
      $.MENU,
      $.META,
      $.NAV,
      $.NOEMBED,
      $.NOFRAMES,
      $.NOSCRIPT,
      $.OBJECT,
      $.OL,
      $.P,
      $.PARAM,
      $.PLAINTEXT,
      $.PRE,
      $.SCRIPT,
      $.SECTION,
      $.SELECT,
      $.SOURCE,
      $.STYLE,
      $.SUMMARY,
      $.TABLE,
      $.TBODY,
      $.TD,
      $.TEMPLATE,
      $.TEXTAREA,
      $.TFOOT,
      $.TH,
      $.THEAD,
      $.TITLE,
      $.TR,
      $.TRACK,
      $.UL,
      $.WBR,
      $.XMP
    ]),
    ["http://www.w3.org/1998/Math/MathML" /* MATHML */]: /* @__PURE__ */ new Set([$.MI, $.MO, $.MN, $.MS, $.MTEXT, $.ANNOTATION_XML]),
    ["http://www.w3.org/2000/svg" /* SVG */]: /* @__PURE__ */ new Set([$.TITLE, $.FOREIGN_OBJECT, $.DESC]),
    ["http://www.w3.org/1999/xlink" /* XLINK */]: /* @__PURE__ */ new Set(),
    ["http://www.w3.org/XML/1998/namespace" /* XML */]: /* @__PURE__ */ new Set(),
    ["http://www.w3.org/2000/xmlns/" /* XMLNS */]: /* @__PURE__ */ new Set()
  };
  var NUMBERED_HEADERS = /* @__PURE__ */ new Set([$.H1, $.H2, $.H3, $.H4, $.H5, $.H6]);
  var UNESCAPED_TEXT = /* @__PURE__ */ new Set([
    "style" /* STYLE */,
    "script" /* SCRIPT */,
    "xmp" /* XMP */,
    "iframe" /* IFRAME */,
    "noembed" /* NOEMBED */,
    "noframes" /* NOFRAMES */,
    "plaintext" /* PLAINTEXT */
  ]);
  function hasUnescapedText(tn, scriptingEnabled) {
    return UNESCAPED_TEXT.has(tn) || scriptingEnabled && tn === "noscript" /* NOSCRIPT */;
  }

  // src/tokenizer/index.ts
  var TokenizerMode = {
    DATA: 0 /* DATA */,
    RCDATA: 1 /* RCDATA */,
    RAWTEXT: 2 /* RAWTEXT */,
    SCRIPT_DATA: 3 /* SCRIPT_DATA */,
    PLAINTEXT: 4 /* PLAINTEXT */,
    CDATA_SECTION: 68 /* CDATA_SECTION */
  };
  function isAsciiDigit(cp) {
    return cp >= 48 /* DIGIT_0 */ && cp <= 57 /* DIGIT_9 */;
  }
  function isAsciiUpper(cp) {
    return cp >= 65 /* LATIN_CAPITAL_A */ && cp <= 90 /* LATIN_CAPITAL_Z */;
  }
  function isAsciiLower(cp) {
    return cp >= 97 /* LATIN_SMALL_A */ && cp <= 122 /* LATIN_SMALL_Z */;
  }
  function isAsciiLetter(cp) {
    return isAsciiLower(cp) || isAsciiUpper(cp);
  }
  function isAsciiAlphaNumeric(cp) {
    return isAsciiLetter(cp) || isAsciiDigit(cp);
  }
  function toAsciiLower(cp) {
    return cp + 32;
  }
  function isWhitespace(cp) {
    return cp === 32 /* SPACE */ || cp === 10 /* LINE_FEED */ || cp === 9 /* TABULATION */ || cp === 12 /* FORM_FEED */ || cp === 13 /* CARRIAGE_RETURN */;
  }
  function isScriptDataDoubleEscapeSequenceEnd(cp) {
    return isWhitespace(cp) || cp === 47 /* SOLIDUS */ || cp === 62 /* GREATER_THAN_SIGN */;
  }
  function getErrorForNumericCharacterReference(code) {
    if (code === 0 /* NULL */) {
      return "null-character-reference" /* nullCharacterReference */;
    } else if (code > 1114111) {
      return "character-reference-outside-unicode-range" /* characterReferenceOutsideUnicodeRange */;
    } else if (isSurrogate(code)) {
      return "surrogate-character-reference" /* surrogateCharacterReference */;
    } else if (isUndefinedCodePoint(code)) {
      return "noncharacter-character-reference" /* noncharacterCharacterReference */;
    } else if (isControlCodePoint(code) || code === 13 /* CARRIAGE_RETURN */) {
      return "control-character-reference" /* controlCharacterReference */;
    }
    return null;
  }
  var Tokenizer = class {
    constructor(options, handler) {
      this.options = options;
      this.handler = handler;
      __publicField(this, "preprocessor");
      __publicField(this, "paused", false);
      /** Ensures that the parsing loop isn't run multiple times at once. */
      __publicField(this, "inLoop", false);
      /**
       * Indicates that the current adjusted node exists, is not an element in the HTML namespace,
       * and that it is not an integration point for either MathML or HTML.
       *
       * @see {@link https://html.spec.whatwg.org/multipage/parsing.html#tree-construction}
       */
      __publicField(this, "inForeignNode", false);
      __publicField(this, "lastStartTagName", "");
      __publicField(this, "active", false);
      __publicField(this, "state", 0 /* DATA */);
      __publicField(this, "returnState", 0 /* DATA */);
      /**
       * We use `entities`' `EntityDecoder` to parse character references.
       *
       * All of the following states are handled by the `EntityDecoder`:
       *
       * - Named character reference state
       * - Numeric character reference state
       * - Hexademical character reference start state
       * - Hexademical character reference state
       * - Decimal character reference state
       * - Numeric character reference end state
       */
      __publicField(this, "entityDecoder");
      __publicField(this, "entityStartPos", 0);
      __publicField(this, "consumedAfterSnapshot", -1);
      __publicField(this, "currentLocation");
      __publicField(this, "currentCharacterToken", null);
      __publicField(this, "currentToken", null);
      __publicField(this, "currentAttr", { name: "", value: "" });
      this.preprocessor = new Preprocessor(handler);
      this.currentLocation = this.getCurrentLocation(-1);
      this.entityDecoder = new EntityDecoder(
        htmlDecodeTree,
        (cp, consumed) => {
          this.preprocessor.pos = this.entityStartPos + consumed - 1;
          this._flushCodePointConsumedAsCharacterReference(cp);
        },
        handler.onParseError ? {
          missingSemicolonAfterCharacterReference: () => {
            this._err("missing-semicolon-after-character-reference" /* missingSemicolonAfterCharacterReference */, 1);
          },
          absenceOfDigitsInNumericCharacterReference: (consumed) => {
            this._err(
              "absence-of-digits-in-numeric-character-reference" /* absenceOfDigitsInNumericCharacterReference */,
              this.entityStartPos - this.preprocessor.pos + consumed
            );
          },
          validateNumericCharacterReference: (code) => {
            const error = getErrorForNumericCharacterReference(code);
            if (error) this._err(error, 1);
          }
        } : void 0
      );
    }
    //Errors
    _err(code, cpOffset = 0) {
      this.handler.onParseError?.(this.preprocessor.getError(code, cpOffset));
    }
    // NOTE: `offset` may never run across line boundaries.
    getCurrentLocation(offset) {
      if (!this.options.sourceCodeLocationInfo) {
        return null;
      }
      return {
        startLine: this.preprocessor.line,
        startCol: this.preprocessor.col - offset,
        startOffset: this.preprocessor.offset - offset,
        endLine: -1,
        endCol: -1,
        endOffset: -1
      };
    }
    _runParsingLoop() {
      if (this.inLoop) return;
      this.inLoop = true;
      while (this.active && !this.paused) {
        this.consumedAfterSnapshot = 0;
        const cp = this._consume();
        if (!this._ensureHibernation()) {
          this._callState(cp);
        }
      }
      this.inLoop = false;
    }
    //API
    pause() {
      this.paused = true;
    }
    resume(writeCallback) {
      if (!this.paused) {
        throw new Error("Parser was already resumed");
      }
      this.paused = false;
      if (this.inLoop) return;
      this._runParsingLoop();
      if (!this.paused) {
        writeCallback?.();
      }
    }
    write(chunk, isLastChunk, writeCallback) {
      this.active = true;
      this.preprocessor.write(chunk, isLastChunk);
      this._runParsingLoop();
      if (!this.paused) {
        writeCallback?.();
      }
    }
    insertHtmlAtCurrentPos(chunk) {
      this.active = true;
      this.preprocessor.insertHtmlAtCurrentPos(chunk);
      this._runParsingLoop();
    }
    //Hibernation
    _ensureHibernation() {
      if (this.preprocessor.endOfChunkHit) {
        this.preprocessor.retreat(this.consumedAfterSnapshot);
        this.consumedAfterSnapshot = 0;
        this.active = false;
        return true;
      }
      return false;
    }
    //Consumption
    _consume() {
      this.consumedAfterSnapshot++;
      return this.preprocessor.advance();
    }
    _advanceBy(count) {
      this.consumedAfterSnapshot += count;
      for (let i = 0; i < count; i++) {
        this.preprocessor.advance();
      }
    }
    _consumeSequenceIfMatch(pattern, caseSensitive) {
      if (this.preprocessor.startsWith(pattern, caseSensitive)) {
        this._advanceBy(pattern.length - 1);
        return true;
      }
      return false;
    }
    //Token creation
    _createStartTagToken() {
      this.currentToken = {
        type: 3 /* START_TAG */,
        tagName: "",
        tagID: 0 /* UNKNOWN */,
        selfClosing: false,
        ackSelfClosing: false,
        attrs: [],
        location: this.getCurrentLocation(1)
      };
    }
    _createEndTagToken() {
      this.currentToken = {
        type: 4 /* END_TAG */,
        tagName: "",
        tagID: 0 /* UNKNOWN */,
        selfClosing: false,
        ackSelfClosing: false,
        attrs: [],
        location: this.getCurrentLocation(2)
      };
    }
    _createCommentToken(offset) {
      this.currentToken = {
        type: 5 /* COMMENT */,
        data: "",
        location: this.getCurrentLocation(offset)
      };
    }
    _createDoctypeToken(initialName) {
      this.currentToken = {
        type: 6 /* DOCTYPE */,
        name: initialName,
        forceQuirks: false,
        publicId: null,
        systemId: null,
        location: this.currentLocation
      };
    }
    _createCharacterToken(type, chars) {
      this.currentCharacterToken = {
        type,
        chars,
        location: this.currentLocation
      };
    }
    //Tag attributes
    _createAttr(attrNameFirstCh) {
      this.currentAttr = {
        name: attrNameFirstCh,
        value: ""
      };
      this.currentLocation = this.getCurrentLocation(0);
    }
    _leaveAttrName() {
      var _a;
      const token = this.currentToken;
      if (getTokenAttr(token, this.currentAttr.name) === null) {
        token.attrs.push(this.currentAttr);
        if (token.location && this.currentLocation) {
          const attrLocations = (_a = token.location).attrs ?? (_a.attrs = /* @__PURE__ */ Object.create(null));
          attrLocations[this.currentAttr.name] = this.currentLocation;
          this._leaveAttrValue();
        }
      } else {
        this._err("duplicate-attribute" /* duplicateAttribute */);
      }
    }
    _leaveAttrValue() {
      if (this.currentLocation) {
        this.currentLocation.endLine = this.preprocessor.line;
        this.currentLocation.endCol = this.preprocessor.col;
        this.currentLocation.endOffset = this.preprocessor.offset;
      }
    }
    //Token emission
    prepareToken(ct) {
      this._emitCurrentCharacterToken(ct.location);
      this.currentToken = null;
      if (ct.location) {
        ct.location.endLine = this.preprocessor.line;
        ct.location.endCol = this.preprocessor.col + 1;
        ct.location.endOffset = this.preprocessor.offset + 1;
      }
      this.currentLocation = this.getCurrentLocation(-1);
    }
    emitCurrentTagToken() {
      const ct = this.currentToken;
      this.prepareToken(ct);
      ct.tagID = getTagID(ct.tagName);
      if (ct.type === 3 /* START_TAG */) {
        this.lastStartTagName = ct.tagName;
        this.handler.onStartTag(ct);
      } else {
        if (ct.attrs.length > 0) {
          this._err("end-tag-with-attributes" /* endTagWithAttributes */);
        }
        if (ct.selfClosing) {
          this._err("end-tag-with-trailing-solidus" /* endTagWithTrailingSolidus */);
        }
        this.handler.onEndTag(ct);
      }
      this.preprocessor.dropParsedChunk();
    }
    emitCurrentComment(ct) {
      this.prepareToken(ct);
      this.handler.onComment(ct);
      this.preprocessor.dropParsedChunk();
    }
    emitCurrentDoctype(ct) {
      this.prepareToken(ct);
      this.handler.onDoctype(ct);
      this.preprocessor.dropParsedChunk();
    }
    _emitCurrentCharacterToken(nextLocation) {
      if (this.currentCharacterToken) {
        if (nextLocation && this.currentCharacterToken.location) {
          this.currentCharacterToken.location.endLine = nextLocation.startLine;
          this.currentCharacterToken.location.endCol = nextLocation.startCol;
          this.currentCharacterToken.location.endOffset = nextLocation.startOffset;
        }
        switch (this.currentCharacterToken.type) {
          case 0 /* CHARACTER */: {
            this.handler.onCharacter(this.currentCharacterToken);
            break;
          }
          case 1 /* NULL_CHARACTER */: {
            this.handler.onNullCharacter(this.currentCharacterToken);
            break;
          }
          case 2 /* WHITESPACE_CHARACTER */: {
            this.handler.onWhitespaceCharacter(this.currentCharacterToken);
            break;
          }
        }
        this.currentCharacterToken = null;
      }
    }
    _emitEOFToken() {
      const location = this.getCurrentLocation(0);
      if (location) {
        location.endLine = location.startLine;
        location.endCol = location.startCol;
        location.endOffset = location.startOffset;
      }
      this._emitCurrentCharacterToken(location);
      this.handler.onEof({ type: 7 /* EOF */, location });
      this.active = false;
    }
    //Characters emission
    //OPTIMIZATION: The specification uses only one type of character token (one token per character).
    //This causes a huge memory overhead and a lot of unnecessary parser loops. parse5 uses 3 groups of characters.
    //If we have a sequence of characters that belong to the same group, the parser can process it
    //as a single solid character token.
    //So, there are 3 types of character tokens in parse5:
    //1)TokenType.NULL_CHARACTER - \u0000-character sequences (e.g. '\u0000\u0000\u0000')
    //2)TokenType.WHITESPACE_CHARACTER - any whitespace/new-line character sequences (e.g. '\n  \r\t   \f')
    //3)TokenType.CHARACTER - any character sequence which don't belong to groups 1 and 2 (e.g. 'abcdef1234@@#$%^')
    _appendCharToCurrentCharacterToken(type, ch) {
      if (this.currentCharacterToken) {
        if (this.currentCharacterToken.type === type) {
          this.currentCharacterToken.chars += ch;
          return;
        } else {
          this.currentLocation = this.getCurrentLocation(0);
          this._emitCurrentCharacterToken(this.currentLocation);
          this.preprocessor.dropParsedChunk();
        }
      }
      this._createCharacterToken(type, ch);
    }
    _emitCodePoint(cp) {
      const type = isWhitespace(cp) ? 2 /* WHITESPACE_CHARACTER */ : cp === 0 /* NULL */ ? 1 /* NULL_CHARACTER */ : 0 /* CHARACTER */;
      this._appendCharToCurrentCharacterToken(
        type,
        cp < 65536 ? String.fromCharCode(cp) : String.fromCodePoint(cp)
      );
    }
    //NOTE: used when we emit characters explicitly.
    //This is always for non-whitespace and non-null characters, which allows us to avoid additional checks.
    _emitChars(ch) {
      this._appendCharToCurrentCharacterToken(0 /* CHARACTER */, ch);
    }
    // Character reference helpers
    _startCharacterReference() {
      this.returnState = this.state;
      this.state = 71 /* CHARACTER_REFERENCE */;
      this.entityStartPos = this.preprocessor.pos;
      this.entityDecoder.startEntity(
        this._isCharacterReferenceInAttribute() ? DecodingMode.Attribute : DecodingMode.Legacy
      );
    }
    _isCharacterReferenceInAttribute() {
      return this.returnState === 35 /* ATTRIBUTE_VALUE_DOUBLE_QUOTED */ || this.returnState === 36 /* ATTRIBUTE_VALUE_SINGLE_QUOTED */ || this.returnState === 37 /* ATTRIBUTE_VALUE_UNQUOTED */;
    }
    _flushCodePointConsumedAsCharacterReference(cp) {
      if (this._isCharacterReferenceInAttribute()) {
        this.currentAttr.value += String.fromCodePoint(cp);
      } else {
        this._emitCodePoint(cp);
      }
    }
    // Calling states this way turns out to be much faster than any other approach.
    _callState(cp) {
      switch (this.state) {
        case 0 /* DATA */: {
          this._stateData(cp);
          break;
        }
        case 1 /* RCDATA */: {
          this._stateRcdata(cp);
          break;
        }
        case 2 /* RAWTEXT */: {
          this._stateRawtext(cp);
          break;
        }
        case 3 /* SCRIPT_DATA */: {
          this._stateScriptData(cp);
          break;
        }
        case 4 /* PLAINTEXT */: {
          this._statePlaintext(cp);
          break;
        }
        case 5 /* TAG_OPEN */: {
          this._stateTagOpen(cp);
          break;
        }
        case 6 /* END_TAG_OPEN */: {
          this._stateEndTagOpen(cp);
          break;
        }
        case 7 /* TAG_NAME */: {
          this._stateTagName(cp);
          break;
        }
        case 8 /* RCDATA_LESS_THAN_SIGN */: {
          this._stateRcdataLessThanSign(cp);
          break;
        }
        case 9 /* RCDATA_END_TAG_OPEN */: {
          this._stateRcdataEndTagOpen(cp);
          break;
        }
        case 10 /* RCDATA_END_TAG_NAME */: {
          this._stateRcdataEndTagName(cp);
          break;
        }
        case 11 /* RAWTEXT_LESS_THAN_SIGN */: {
          this._stateRawtextLessThanSign(cp);
          break;
        }
        case 12 /* RAWTEXT_END_TAG_OPEN */: {
          this._stateRawtextEndTagOpen(cp);
          break;
        }
        case 13 /* RAWTEXT_END_TAG_NAME */: {
          this._stateRawtextEndTagName(cp);
          break;
        }
        case 14 /* SCRIPT_DATA_LESS_THAN_SIGN */: {
          this._stateScriptDataLessThanSign(cp);
          break;
        }
        case 15 /* SCRIPT_DATA_END_TAG_OPEN */: {
          this._stateScriptDataEndTagOpen(cp);
          break;
        }
        case 16 /* SCRIPT_DATA_END_TAG_NAME */: {
          this._stateScriptDataEndTagName(cp);
          break;
        }
        case 17 /* SCRIPT_DATA_ESCAPE_START */: {
          this._stateScriptDataEscapeStart(cp);
          break;
        }
        case 18 /* SCRIPT_DATA_ESCAPE_START_DASH */: {
          this._stateScriptDataEscapeStartDash(cp);
          break;
        }
        case 19 /* SCRIPT_DATA_ESCAPED */: {
          this._stateScriptDataEscaped(cp);
          break;
        }
        case 20 /* SCRIPT_DATA_ESCAPED_DASH */: {
          this._stateScriptDataEscapedDash(cp);
          break;
        }
        case 21 /* SCRIPT_DATA_ESCAPED_DASH_DASH */: {
          this._stateScriptDataEscapedDashDash(cp);
          break;
        }
        case 22 /* SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN */: {
          this._stateScriptDataEscapedLessThanSign(cp);
          break;
        }
        case 23 /* SCRIPT_DATA_ESCAPED_END_TAG_OPEN */: {
          this._stateScriptDataEscapedEndTagOpen(cp);
          break;
        }
        case 24 /* SCRIPT_DATA_ESCAPED_END_TAG_NAME */: {
          this._stateScriptDataEscapedEndTagName(cp);
          break;
        }
        case 25 /* SCRIPT_DATA_DOUBLE_ESCAPE_START */: {
          this._stateScriptDataDoubleEscapeStart(cp);
          break;
        }
        case 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */: {
          this._stateScriptDataDoubleEscaped(cp);
          break;
        }
        case 27 /* SCRIPT_DATA_DOUBLE_ESCAPED_DASH */: {
          this._stateScriptDataDoubleEscapedDash(cp);
          break;
        }
        case 28 /* SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH */: {
          this._stateScriptDataDoubleEscapedDashDash(cp);
          break;
        }
        case 29 /* SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN */: {
          this._stateScriptDataDoubleEscapedLessThanSign(cp);
          break;
        }
        case 30 /* SCRIPT_DATA_DOUBLE_ESCAPE_END */: {
          this._stateScriptDataDoubleEscapeEnd(cp);
          break;
        }
        case 31 /* BEFORE_ATTRIBUTE_NAME */: {
          this._stateBeforeAttributeName(cp);
          break;
        }
        case 32 /* ATTRIBUTE_NAME */: {
          this._stateAttributeName(cp);
          break;
        }
        case 33 /* AFTER_ATTRIBUTE_NAME */: {
          this._stateAfterAttributeName(cp);
          break;
        }
        case 34 /* BEFORE_ATTRIBUTE_VALUE */: {
          this._stateBeforeAttributeValue(cp);
          break;
        }
        case 35 /* ATTRIBUTE_VALUE_DOUBLE_QUOTED */: {
          this._stateAttributeValueDoubleQuoted(cp);
          break;
        }
        case 36 /* ATTRIBUTE_VALUE_SINGLE_QUOTED */: {
          this._stateAttributeValueSingleQuoted(cp);
          break;
        }
        case 37 /* ATTRIBUTE_VALUE_UNQUOTED */: {
          this._stateAttributeValueUnquoted(cp);
          break;
        }
        case 38 /* AFTER_ATTRIBUTE_VALUE_QUOTED */: {
          this._stateAfterAttributeValueQuoted(cp);
          break;
        }
        case 39 /* SELF_CLOSING_START_TAG */: {
          this._stateSelfClosingStartTag(cp);
          break;
        }
        case 40 /* BOGUS_COMMENT */: {
          this._stateBogusComment(cp);
          break;
        }
        case 41 /* MARKUP_DECLARATION_OPEN */: {
          this._stateMarkupDeclarationOpen(cp);
          break;
        }
        case 42 /* COMMENT_START */: {
          this._stateCommentStart(cp);
          break;
        }
        case 43 /* COMMENT_START_DASH */: {
          this._stateCommentStartDash(cp);
          break;
        }
        case 44 /* COMMENT */: {
          this._stateComment(cp);
          break;
        }
        case 45 /* COMMENT_LESS_THAN_SIGN */: {
          this._stateCommentLessThanSign(cp);
          break;
        }
        case 46 /* COMMENT_LESS_THAN_SIGN_BANG */: {
          this._stateCommentLessThanSignBang(cp);
          break;
        }
        case 47 /* COMMENT_LESS_THAN_SIGN_BANG_DASH */: {
          this._stateCommentLessThanSignBangDash(cp);
          break;
        }
        case 48 /* COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH */: {
          this._stateCommentLessThanSignBangDashDash(cp);
          break;
        }
        case 49 /* COMMENT_END_DASH */: {
          this._stateCommentEndDash(cp);
          break;
        }
        case 50 /* COMMENT_END */: {
          this._stateCommentEnd(cp);
          break;
        }
        case 51 /* COMMENT_END_BANG */: {
          this._stateCommentEndBang(cp);
          break;
        }
        case 52 /* DOCTYPE */: {
          this._stateDoctype(cp);
          break;
        }
        case 53 /* BEFORE_DOCTYPE_NAME */: {
          this._stateBeforeDoctypeName(cp);
          break;
        }
        case 54 /* DOCTYPE_NAME */: {
          this._stateDoctypeName(cp);
          break;
        }
        case 55 /* AFTER_DOCTYPE_NAME */: {
          this._stateAfterDoctypeName(cp);
          break;
        }
        case 56 /* AFTER_DOCTYPE_PUBLIC_KEYWORD */: {
          this._stateAfterDoctypePublicKeyword(cp);
          break;
        }
        case 57 /* BEFORE_DOCTYPE_PUBLIC_IDENTIFIER */: {
          this._stateBeforeDoctypePublicIdentifier(cp);
          break;
        }
        case 58 /* DOCTYPE_PUBLIC_IDENTIFIER_DOUBLE_QUOTED */: {
          this._stateDoctypePublicIdentifierDoubleQuoted(cp);
          break;
        }
        case 59 /* DOCTYPE_PUBLIC_IDENTIFIER_SINGLE_QUOTED */: {
          this._stateDoctypePublicIdentifierSingleQuoted(cp);
          break;
        }
        case 60 /* AFTER_DOCTYPE_PUBLIC_IDENTIFIER */: {
          this._stateAfterDoctypePublicIdentifier(cp);
          break;
        }
        case 61 /* BETWEEN_DOCTYPE_PUBLIC_AND_SYSTEM_IDENTIFIERS */: {
          this._stateBetweenDoctypePublicAndSystemIdentifiers(cp);
          break;
        }
        case 62 /* AFTER_DOCTYPE_SYSTEM_KEYWORD */: {
          this._stateAfterDoctypeSystemKeyword(cp);
          break;
        }
        case 63 /* BEFORE_DOCTYPE_SYSTEM_IDENTIFIER */: {
          this._stateBeforeDoctypeSystemIdentifier(cp);
          break;
        }
        case 64 /* DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED */: {
          this._stateDoctypeSystemIdentifierDoubleQuoted(cp);
          break;
        }
        case 65 /* DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED */: {
          this._stateDoctypeSystemIdentifierSingleQuoted(cp);
          break;
        }
        case 66 /* AFTER_DOCTYPE_SYSTEM_IDENTIFIER */: {
          this._stateAfterDoctypeSystemIdentifier(cp);
          break;
        }
        case 67 /* BOGUS_DOCTYPE */: {
          this._stateBogusDoctype(cp);
          break;
        }
        case 68 /* CDATA_SECTION */: {
          this._stateCdataSection(cp);
          break;
        }
        case 69 /* CDATA_SECTION_BRACKET */: {
          this._stateCdataSectionBracket(cp);
          break;
        }
        case 70 /* CDATA_SECTION_END */: {
          this._stateCdataSectionEnd(cp);
          break;
        }
        case 71 /* CHARACTER_REFERENCE */: {
          this._stateCharacterReference();
          break;
        }
        case 72 /* AMBIGUOUS_AMPERSAND */: {
          this._stateAmbiguousAmpersand(cp);
          break;
        }
        default: {
          throw new Error("Unknown state");
        }
      }
    }
    // State machine
    // Data state
    //------------------------------------------------------------------
    _stateData(cp) {
      switch (cp) {
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 5 /* TAG_OPEN */;
          break;
        }
        case 38 /* AMPERSAND */: {
          this._startCharacterReference();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitCodePoint(cp);
          break;
        }
        case -1 /* EOF */: {
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    //  RCDATA state
    //------------------------------------------------------------------
    _stateRcdata(cp) {
      switch (cp) {
        case 38 /* AMPERSAND */: {
          this._startCharacterReference();
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 8 /* RCDATA_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // RAWTEXT state
    //------------------------------------------------------------------
    _stateRawtext(cp) {
      switch (cp) {
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 11 /* RAWTEXT_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data state
    //------------------------------------------------------------------
    _stateScriptData(cp) {
      switch (cp) {
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 14 /* SCRIPT_DATA_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // PLAINTEXT state
    //------------------------------------------------------------------
    _statePlaintext(cp) {
      switch (cp) {
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // Tag open state
    //------------------------------------------------------------------
    _stateTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this._createStartTagToken();
        this.state = 7 /* TAG_NAME */;
        this._stateTagName(cp);
      } else
        switch (cp) {
          case 33 /* EXCLAMATION_MARK */: {
            this.state = 41 /* MARKUP_DECLARATION_OPEN */;
            break;
          }
          case 47 /* SOLIDUS */: {
            this.state = 6 /* END_TAG_OPEN */;
            break;
          }
          case 63 /* QUESTION_MARK */: {
            this._err("unexpected-question-mark-instead-of-tag-name" /* unexpectedQuestionMarkInsteadOfTagName */);
            this._createCommentToken(1);
            this.state = 40 /* BOGUS_COMMENT */;
            this._stateBogusComment(cp);
            break;
          }
          case -1 /* EOF */: {
            this._err("eof-before-tag-name" /* eofBeforeTagName */);
            this._emitChars("<");
            this._emitEOFToken();
            break;
          }
          default: {
            this._err("invalid-first-character-of-tag-name" /* invalidFirstCharacterOfTagName */);
            this._emitChars("<");
            this.state = 0 /* DATA */;
            this._stateData(cp);
          }
        }
    }
    // End tag open state
    //------------------------------------------------------------------
    _stateEndTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this._createEndTagToken();
        this.state = 7 /* TAG_NAME */;
        this._stateTagName(cp);
      } else
        switch (cp) {
          case 62 /* GREATER_THAN_SIGN */: {
            this._err("missing-end-tag-name" /* missingEndTagName */);
            this.state = 0 /* DATA */;
            break;
          }
          case -1 /* EOF */: {
            this._err("eof-before-tag-name" /* eofBeforeTagName */);
            this._emitChars("</");
            this._emitEOFToken();
            break;
          }
          default: {
            this._err("invalid-first-character-of-tag-name" /* invalidFirstCharacterOfTagName */);
            this._createCommentToken(2);
            this.state = 40 /* BOGUS_COMMENT */;
            this._stateBogusComment(cp);
          }
        }
    }
    // Tag name state
    //------------------------------------------------------------------
    _stateTagName(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          break;
        }
        case 47 /* SOLIDUS */: {
          this.state = 39 /* SELF_CLOSING_START_TAG */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.tagName += REPLACEMENT_CHARACTER;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          token.tagName += String.fromCodePoint(isAsciiUpper(cp) ? toAsciiLower(cp) : cp);
        }
      }
    }
    // RCDATA less-than sign state
    //------------------------------------------------------------------
    _stateRcdataLessThanSign(cp) {
      if (cp === 47 /* SOLIDUS */) {
        this.state = 9 /* RCDATA_END_TAG_OPEN */;
      } else {
        this._emitChars("<");
        this.state = 1 /* RCDATA */;
        this._stateRcdata(cp);
      }
    }
    // RCDATA end tag open state
    //------------------------------------------------------------------
    _stateRcdataEndTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this.state = 10 /* RCDATA_END_TAG_NAME */;
        this._stateRcdataEndTagName(cp);
      } else {
        this._emitChars("</");
        this.state = 1 /* RCDATA */;
        this._stateRcdata(cp);
      }
    }
    handleSpecialEndTag(_cp) {
      if (!this.preprocessor.startsWith(this.lastStartTagName, false)) {
        return !this._ensureHibernation();
      }
      this._createEndTagToken();
      const token = this.currentToken;
      token.tagName = this.lastStartTagName;
      const cp = this.preprocessor.peek(this.lastStartTagName.length);
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this._advanceBy(this.lastStartTagName.length);
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          return false;
        }
        case 47 /* SOLIDUS */: {
          this._advanceBy(this.lastStartTagName.length);
          this.state = 39 /* SELF_CLOSING_START_TAG */;
          return false;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._advanceBy(this.lastStartTagName.length);
          this.emitCurrentTagToken();
          this.state = 0 /* DATA */;
          return false;
        }
        default: {
          return !this._ensureHibernation();
        }
      }
    }
    // RCDATA end tag name state
    //------------------------------------------------------------------
    _stateRcdataEndTagName(cp) {
      if (this.handleSpecialEndTag(cp)) {
        this._emitChars("</");
        this.state = 1 /* RCDATA */;
        this._stateRcdata(cp);
      }
    }
    // RAWTEXT less-than sign state
    //------------------------------------------------------------------
    _stateRawtextLessThanSign(cp) {
      if (cp === 47 /* SOLIDUS */) {
        this.state = 12 /* RAWTEXT_END_TAG_OPEN */;
      } else {
        this._emitChars("<");
        this.state = 2 /* RAWTEXT */;
        this._stateRawtext(cp);
      }
    }
    // RAWTEXT end tag open state
    //------------------------------------------------------------------
    _stateRawtextEndTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this.state = 13 /* RAWTEXT_END_TAG_NAME */;
        this._stateRawtextEndTagName(cp);
      } else {
        this._emitChars("</");
        this.state = 2 /* RAWTEXT */;
        this._stateRawtext(cp);
      }
    }
    // RAWTEXT end tag name state
    //------------------------------------------------------------------
    _stateRawtextEndTagName(cp) {
      if (this.handleSpecialEndTag(cp)) {
        this._emitChars("</");
        this.state = 2 /* RAWTEXT */;
        this._stateRawtext(cp);
      }
    }
    // Script data less-than sign state
    //------------------------------------------------------------------
    _stateScriptDataLessThanSign(cp) {
      switch (cp) {
        case 47 /* SOLIDUS */: {
          this.state = 15 /* SCRIPT_DATA_END_TAG_OPEN */;
          break;
        }
        case 33 /* EXCLAMATION_MARK */: {
          this.state = 17 /* SCRIPT_DATA_ESCAPE_START */;
          this._emitChars("<!");
          break;
        }
        default: {
          this._emitChars("<");
          this.state = 3 /* SCRIPT_DATA */;
          this._stateScriptData(cp);
        }
      }
    }
    // Script data end tag open state
    //------------------------------------------------------------------
    _stateScriptDataEndTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this.state = 16 /* SCRIPT_DATA_END_TAG_NAME */;
        this._stateScriptDataEndTagName(cp);
      } else {
        this._emitChars("</");
        this.state = 3 /* SCRIPT_DATA */;
        this._stateScriptData(cp);
      }
    }
    // Script data end tag name state
    //------------------------------------------------------------------
    _stateScriptDataEndTagName(cp) {
      if (this.handleSpecialEndTag(cp)) {
        this._emitChars("</");
        this.state = 3 /* SCRIPT_DATA */;
        this._stateScriptData(cp);
      }
    }
    // Script data escape start state
    //------------------------------------------------------------------
    _stateScriptDataEscapeStart(cp) {
      if (cp === 45 /* HYPHEN_MINUS */) {
        this.state = 18 /* SCRIPT_DATA_ESCAPE_START_DASH */;
        this._emitChars("-");
      } else {
        this.state = 3 /* SCRIPT_DATA */;
        this._stateScriptData(cp);
      }
    }
    // Script data escape start dash state
    //------------------------------------------------------------------
    _stateScriptDataEscapeStartDash(cp) {
      if (cp === 45 /* HYPHEN_MINUS */) {
        this.state = 21 /* SCRIPT_DATA_ESCAPED_DASH_DASH */;
        this._emitChars("-");
      } else {
        this.state = 3 /* SCRIPT_DATA */;
        this._stateScriptData(cp);
      }
    }
    // Script data escaped state
    //------------------------------------------------------------------
    _stateScriptDataEscaped(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 20 /* SCRIPT_DATA_ESCAPED_DASH */;
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 22 /* SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data escaped dash state
    //------------------------------------------------------------------
    _stateScriptDataEscapedDash(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 21 /* SCRIPT_DATA_ESCAPED_DASH_DASH */;
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 22 /* SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.state = 19 /* SCRIPT_DATA_ESCAPED */;
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.state = 19 /* SCRIPT_DATA_ESCAPED */;
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data escaped dash dash state
    //------------------------------------------------------------------
    _stateScriptDataEscapedDashDash(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 22 /* SCRIPT_DATA_ESCAPED_LESS_THAN_SIGN */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 3 /* SCRIPT_DATA */;
          this._emitChars(">");
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.state = 19 /* SCRIPT_DATA_ESCAPED */;
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.state = 19 /* SCRIPT_DATA_ESCAPED */;
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data escaped less-than sign state
    //------------------------------------------------------------------
    _stateScriptDataEscapedLessThanSign(cp) {
      if (cp === 47 /* SOLIDUS */) {
        this.state = 23 /* SCRIPT_DATA_ESCAPED_END_TAG_OPEN */;
      } else if (isAsciiLetter(cp)) {
        this._emitChars("<");
        this.state = 25 /* SCRIPT_DATA_DOUBLE_ESCAPE_START */;
        this._stateScriptDataDoubleEscapeStart(cp);
      } else {
        this._emitChars("<");
        this.state = 19 /* SCRIPT_DATA_ESCAPED */;
        this._stateScriptDataEscaped(cp);
      }
    }
    // Script data escaped end tag open state
    //------------------------------------------------------------------
    _stateScriptDataEscapedEndTagOpen(cp) {
      if (isAsciiLetter(cp)) {
        this.state = 24 /* SCRIPT_DATA_ESCAPED_END_TAG_NAME */;
        this._stateScriptDataEscapedEndTagName(cp);
      } else {
        this._emitChars("</");
        this.state = 19 /* SCRIPT_DATA_ESCAPED */;
        this._stateScriptDataEscaped(cp);
      }
    }
    // Script data escaped end tag name state
    //------------------------------------------------------------------
    _stateScriptDataEscapedEndTagName(cp) {
      if (this.handleSpecialEndTag(cp)) {
        this._emitChars("</");
        this.state = 19 /* SCRIPT_DATA_ESCAPED */;
        this._stateScriptDataEscaped(cp);
      }
    }
    // Script data double escape start state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscapeStart(cp) {
      if (this.preprocessor.startsWith(SEQUENCES.SCRIPT, false) && isScriptDataDoubleEscapeSequenceEnd(this.preprocessor.peek(SEQUENCES.SCRIPT.length))) {
        this._emitCodePoint(cp);
        for (let i = 0; i < SEQUENCES.SCRIPT.length; i++) {
          this._emitCodePoint(this._consume());
        }
        this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
      } else if (!this._ensureHibernation()) {
        this.state = 19 /* SCRIPT_DATA_ESCAPED */;
        this._stateScriptDataEscaped(cp);
      }
    }
    // Script data double escaped state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscaped(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 27 /* SCRIPT_DATA_DOUBLE_ESCAPED_DASH */;
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 29 /* SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN */;
          this._emitChars("<");
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data double escaped dash state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscapedDash(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 28 /* SCRIPT_DATA_DOUBLE_ESCAPED_DASH_DASH */;
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 29 /* SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN */;
          this._emitChars("<");
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data double escaped dash dash state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscapedDashDash(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this._emitChars("-");
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          this.state = 29 /* SCRIPT_DATA_DOUBLE_ESCAPED_LESS_THAN_SIGN */;
          this._emitChars("<");
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 3 /* SCRIPT_DATA */;
          this._emitChars(">");
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
          this._emitChars(REPLACEMENT_CHARACTER);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-script-html-comment-like-text" /* eofInScriptHtmlCommentLikeText */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
          this._emitCodePoint(cp);
        }
      }
    }
    // Script data double escaped less-than sign state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscapedLessThanSign(cp) {
      if (cp === 47 /* SOLIDUS */) {
        this.state = 30 /* SCRIPT_DATA_DOUBLE_ESCAPE_END */;
        this._emitChars("/");
      } else {
        this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
        this._stateScriptDataDoubleEscaped(cp);
      }
    }
    // Script data double escape end state
    //------------------------------------------------------------------
    _stateScriptDataDoubleEscapeEnd(cp) {
      if (this.preprocessor.startsWith(SEQUENCES.SCRIPT, false) && isScriptDataDoubleEscapeSequenceEnd(this.preprocessor.peek(SEQUENCES.SCRIPT.length))) {
        this._emitCodePoint(cp);
        for (let i = 0; i < SEQUENCES.SCRIPT.length; i++) {
          this._emitCodePoint(this._consume());
        }
        this.state = 19 /* SCRIPT_DATA_ESCAPED */;
      } else if (!this._ensureHibernation()) {
        this.state = 26 /* SCRIPT_DATA_DOUBLE_ESCAPED */;
        this._stateScriptDataDoubleEscaped(cp);
      }
    }
    // Before attribute name state
    //------------------------------------------------------------------
    _stateBeforeAttributeName(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 47 /* SOLIDUS */:
        case 62 /* GREATER_THAN_SIGN */:
        case -1 /* EOF */: {
          this.state = 33 /* AFTER_ATTRIBUTE_NAME */;
          this._stateAfterAttributeName(cp);
          break;
        }
        case 61 /* EQUALS_SIGN */: {
          this._err("unexpected-equals-sign-before-attribute-name" /* unexpectedEqualsSignBeforeAttributeName */);
          this._createAttr("=");
          this.state = 32 /* ATTRIBUTE_NAME */;
          break;
        }
        default: {
          this._createAttr("");
          this.state = 32 /* ATTRIBUTE_NAME */;
          this._stateAttributeName(cp);
        }
      }
    }
    // Attribute name state
    //------------------------------------------------------------------
    _stateAttributeName(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */:
        case 47 /* SOLIDUS */:
        case 62 /* GREATER_THAN_SIGN */:
        case -1 /* EOF */: {
          this._leaveAttrName();
          this.state = 33 /* AFTER_ATTRIBUTE_NAME */;
          this._stateAfterAttributeName(cp);
          break;
        }
        case 61 /* EQUALS_SIGN */: {
          this._leaveAttrName();
          this.state = 34 /* BEFORE_ATTRIBUTE_VALUE */;
          break;
        }
        case 34 /* QUOTATION_MARK */:
        case 39 /* APOSTROPHE */:
        case 60 /* LESS_THAN_SIGN */: {
          this._err("unexpected-character-in-attribute-name" /* unexpectedCharacterInAttributeName */);
          this.currentAttr.name += String.fromCodePoint(cp);
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.currentAttr.name += REPLACEMENT_CHARACTER;
          break;
        }
        default: {
          this.currentAttr.name += String.fromCodePoint(isAsciiUpper(cp) ? toAsciiLower(cp) : cp);
        }
      }
    }
    // After attribute name state
    //------------------------------------------------------------------
    _stateAfterAttributeName(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 47 /* SOLIDUS */: {
          this.state = 39 /* SELF_CLOSING_START_TAG */;
          break;
        }
        case 61 /* EQUALS_SIGN */: {
          this.state = 34 /* BEFORE_ATTRIBUTE_VALUE */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._createAttr("");
          this.state = 32 /* ATTRIBUTE_NAME */;
          this._stateAttributeName(cp);
        }
      }
    }
    // Before attribute value state
    //------------------------------------------------------------------
    _stateBeforeAttributeValue(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          this.state = 35 /* ATTRIBUTE_VALUE_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          this.state = 36 /* ATTRIBUTE_VALUE_SINGLE_QUOTED */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("missing-attribute-value" /* missingAttributeValue */);
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        default: {
          this.state = 37 /* ATTRIBUTE_VALUE_UNQUOTED */;
          this._stateAttributeValueUnquoted(cp);
        }
      }
    }
    // Attribute value (double-quoted) state
    //------------------------------------------------------------------
    _stateAttributeValueDoubleQuoted(cp) {
      switch (cp) {
        case 34 /* QUOTATION_MARK */: {
          this.state = 38 /* AFTER_ATTRIBUTE_VALUE_QUOTED */;
          break;
        }
        case 38 /* AMPERSAND */: {
          this._startCharacterReference();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.currentAttr.value += REPLACEMENT_CHARACTER;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.currentAttr.value += String.fromCodePoint(cp);
        }
      }
    }
    // Attribute value (single-quoted) state
    //------------------------------------------------------------------
    _stateAttributeValueSingleQuoted(cp) {
      switch (cp) {
        case 39 /* APOSTROPHE */: {
          this.state = 38 /* AFTER_ATTRIBUTE_VALUE_QUOTED */;
          break;
        }
        case 38 /* AMPERSAND */: {
          this._startCharacterReference();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.currentAttr.value += REPLACEMENT_CHARACTER;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.currentAttr.value += String.fromCodePoint(cp);
        }
      }
    }
    // Attribute value (unquoted) state
    //------------------------------------------------------------------
    _stateAttributeValueUnquoted(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this._leaveAttrValue();
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          break;
        }
        case 38 /* AMPERSAND */: {
          this._startCharacterReference();
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._leaveAttrValue();
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          this.currentAttr.value += REPLACEMENT_CHARACTER;
          break;
        }
        case 34 /* QUOTATION_MARK */:
        case 39 /* APOSTROPHE */:
        case 60 /* LESS_THAN_SIGN */:
        case 61 /* EQUALS_SIGN */:
        case 96 /* GRAVE_ACCENT */: {
          this._err("unexpected-character-in-unquoted-attribute-value" /* unexpectedCharacterInUnquotedAttributeValue */);
          this.currentAttr.value += String.fromCodePoint(cp);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this.currentAttr.value += String.fromCodePoint(cp);
        }
      }
    }
    // After attribute value (quoted) state
    //------------------------------------------------------------------
    _stateAfterAttributeValueQuoted(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this._leaveAttrValue();
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          break;
        }
        case 47 /* SOLIDUS */: {
          this._leaveAttrValue();
          this.state = 39 /* SELF_CLOSING_START_TAG */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._leaveAttrValue();
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-whitespace-between-attributes" /* missingWhitespaceBetweenAttributes */);
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          this._stateBeforeAttributeName(cp);
        }
      }
    }
    // Self-closing start tag state
    //------------------------------------------------------------------
    _stateSelfClosingStartTag(cp) {
      switch (cp) {
        case 62 /* GREATER_THAN_SIGN */: {
          const token = this.currentToken;
          token.selfClosing = true;
          this.state = 0 /* DATA */;
          this.emitCurrentTagToken();
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-tag" /* eofInTag */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("unexpected-solidus-in-tag" /* unexpectedSolidusInTag */);
          this.state = 31 /* BEFORE_ATTRIBUTE_NAME */;
          this._stateBeforeAttributeName(cp);
        }
      }
    }
    // Bogus comment state
    //------------------------------------------------------------------
    _stateBogusComment(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentComment(token);
          break;
        }
        case -1 /* EOF */: {
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.data += REPLACEMENT_CHARACTER;
          break;
        }
        default: {
          token.data += String.fromCodePoint(cp);
        }
      }
    }
    // Markup declaration open state
    //------------------------------------------------------------------
    _stateMarkupDeclarationOpen(cp) {
      if (this._consumeSequenceIfMatch(SEQUENCES.DASH_DASH, true)) {
        this._createCommentToken(SEQUENCES.DASH_DASH.length + 1);
        this.state = 42 /* COMMENT_START */;
      } else if (this._consumeSequenceIfMatch(SEQUENCES.DOCTYPE, false)) {
        this.currentLocation = this.getCurrentLocation(SEQUENCES.DOCTYPE.length + 1);
        this.state = 52 /* DOCTYPE */;
      } else if (this._consumeSequenceIfMatch(SEQUENCES.CDATA_START, true)) {
        if (this.inForeignNode) {
          this.state = 68 /* CDATA_SECTION */;
        } else {
          this._err("cdata-in-html-content" /* cdataInHtmlContent */);
          this._createCommentToken(SEQUENCES.CDATA_START.length + 1);
          this.currentToken.data = "[CDATA[";
          this.state = 40 /* BOGUS_COMMENT */;
        }
      } else if (!this._ensureHibernation()) {
        this._err("incorrectly-opened-comment" /* incorrectlyOpenedComment */);
        this._createCommentToken(2);
        this.state = 40 /* BOGUS_COMMENT */;
        this._stateBogusComment(cp);
      }
    }
    // Comment start state
    //------------------------------------------------------------------
    _stateCommentStart(cp) {
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 43 /* COMMENT_START_DASH */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-closing-of-empty-comment" /* abruptClosingOfEmptyComment */);
          this.state = 0 /* DATA */;
          const token = this.currentToken;
          this.emitCurrentComment(token);
          break;
        }
        default: {
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // Comment start dash state
    //------------------------------------------------------------------
    _stateCommentStartDash(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 50 /* COMMENT_END */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-closing-of-empty-comment" /* abruptClosingOfEmptyComment */);
          this.state = 0 /* DATA */;
          this.emitCurrentComment(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-comment" /* eofInComment */);
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.data += "-";
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // Comment state
    //------------------------------------------------------------------
    _stateComment(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 49 /* COMMENT_END_DASH */;
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          token.data += "<";
          this.state = 45 /* COMMENT_LESS_THAN_SIGN */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.data += REPLACEMENT_CHARACTER;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-comment" /* eofInComment */);
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.data += String.fromCodePoint(cp);
        }
      }
    }
    // Comment less-than sign state
    //------------------------------------------------------------------
    _stateCommentLessThanSign(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 33 /* EXCLAMATION_MARK */: {
          token.data += "!";
          this.state = 46 /* COMMENT_LESS_THAN_SIGN_BANG */;
          break;
        }
        case 60 /* LESS_THAN_SIGN */: {
          token.data += "<";
          break;
        }
        default: {
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // Comment less-than sign bang state
    //------------------------------------------------------------------
    _stateCommentLessThanSignBang(cp) {
      if (cp === 45 /* HYPHEN_MINUS */) {
        this.state = 47 /* COMMENT_LESS_THAN_SIGN_BANG_DASH */;
      } else {
        this.state = 44 /* COMMENT */;
        this._stateComment(cp);
      }
    }
    // Comment less-than sign bang dash state
    //------------------------------------------------------------------
    _stateCommentLessThanSignBangDash(cp) {
      if (cp === 45 /* HYPHEN_MINUS */) {
        this.state = 48 /* COMMENT_LESS_THAN_SIGN_BANG_DASH_DASH */;
      } else {
        this.state = 49 /* COMMENT_END_DASH */;
        this._stateCommentEndDash(cp);
      }
    }
    // Comment less-than sign bang dash dash state
    //------------------------------------------------------------------
    _stateCommentLessThanSignBangDashDash(cp) {
      if (cp !== 62 /* GREATER_THAN_SIGN */ && cp !== -1 /* EOF */) {
        this._err("nested-comment" /* nestedComment */);
      }
      this.state = 50 /* COMMENT_END */;
      this._stateCommentEnd(cp);
    }
    // Comment end dash state
    //------------------------------------------------------------------
    _stateCommentEndDash(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          this.state = 50 /* COMMENT_END */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-comment" /* eofInComment */);
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.data += "-";
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // Comment end state
    //------------------------------------------------------------------
    _stateCommentEnd(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentComment(token);
          break;
        }
        case 33 /* EXCLAMATION_MARK */: {
          this.state = 51 /* COMMENT_END_BANG */;
          break;
        }
        case 45 /* HYPHEN_MINUS */: {
          token.data += "-";
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-comment" /* eofInComment */);
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.data += "--";
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // Comment end bang state
    //------------------------------------------------------------------
    _stateCommentEndBang(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 45 /* HYPHEN_MINUS */: {
          token.data += "--!";
          this.state = 49 /* COMMENT_END_DASH */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("incorrectly-closed-comment" /* incorrectlyClosedComment */);
          this.state = 0 /* DATA */;
          this.emitCurrentComment(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-comment" /* eofInComment */);
          this.emitCurrentComment(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.data += "--!";
          this.state = 44 /* COMMENT */;
          this._stateComment(cp);
        }
      }
    }
    // DOCTYPE state
    //------------------------------------------------------------------
    _stateDoctype(cp) {
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 53 /* BEFORE_DOCTYPE_NAME */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 53 /* BEFORE_DOCTYPE_NAME */;
          this._stateBeforeDoctypeName(cp);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          this._createDoctypeToken(null);
          const token = this.currentToken;
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-whitespace-before-doctype-name" /* missingWhitespaceBeforeDoctypeName */);
          this.state = 53 /* BEFORE_DOCTYPE_NAME */;
          this._stateBeforeDoctypeName(cp);
        }
      }
    }
    // Before DOCTYPE name state
    //------------------------------------------------------------------
    _stateBeforeDoctypeName(cp) {
      if (isAsciiUpper(cp)) {
        this._createDoctypeToken(String.fromCharCode(toAsciiLower(cp)));
        this.state = 54 /* DOCTYPE_NAME */;
      } else
        switch (cp) {
          case 32 /* SPACE */:
          case 10 /* LINE_FEED */:
          case 9 /* TABULATION */:
          case 12 /* FORM_FEED */: {
            break;
          }
          case 0 /* NULL */: {
            this._err("unexpected-null-character" /* unexpectedNullCharacter */);
            this._createDoctypeToken(REPLACEMENT_CHARACTER);
            this.state = 54 /* DOCTYPE_NAME */;
            break;
          }
          case 62 /* GREATER_THAN_SIGN */: {
            this._err("missing-doctype-name" /* missingDoctypeName */);
            this._createDoctypeToken(null);
            const token = this.currentToken;
            token.forceQuirks = true;
            this.emitCurrentDoctype(token);
            this.state = 0 /* DATA */;
            break;
          }
          case -1 /* EOF */: {
            this._err("eof-in-doctype" /* eofInDoctype */);
            this._createDoctypeToken(null);
            const token = this.currentToken;
            token.forceQuirks = true;
            this.emitCurrentDoctype(token);
            this._emitEOFToken();
            break;
          }
          default: {
            this._createDoctypeToken(String.fromCodePoint(cp));
            this.state = 54 /* DOCTYPE_NAME */;
          }
        }
    }
    // DOCTYPE name state
    //------------------------------------------------------------------
    _stateDoctypeName(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 55 /* AFTER_DOCTYPE_NAME */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.name += REPLACEMENT_CHARACTER;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.name += String.fromCodePoint(isAsciiUpper(cp) ? toAsciiLower(cp) : cp);
        }
      }
    }
    // After DOCTYPE name state
    //------------------------------------------------------------------
    _stateAfterDoctypeName(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          if (this._consumeSequenceIfMatch(SEQUENCES.PUBLIC, false)) {
            this.state = 56 /* AFTER_DOCTYPE_PUBLIC_KEYWORD */;
          } else if (this._consumeSequenceIfMatch(SEQUENCES.SYSTEM, false)) {
            this.state = 62 /* AFTER_DOCTYPE_SYSTEM_KEYWORD */;
          } else if (!this._ensureHibernation()) {
            this._err("invalid-character-sequence-after-doctype-name" /* invalidCharacterSequenceAfterDoctypeName */);
            token.forceQuirks = true;
            this.state = 67 /* BOGUS_DOCTYPE */;
            this._stateBogusDoctype(cp);
          }
        }
      }
    }
    // After DOCTYPE public keyword state
    //------------------------------------------------------------------
    _stateAfterDoctypePublicKeyword(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 57 /* BEFORE_DOCTYPE_PUBLIC_IDENTIFIER */;
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          this._err("missing-whitespace-after-doctype-public-keyword" /* missingWhitespaceAfterDoctypePublicKeyword */);
          token.publicId = "";
          this.state = 58 /* DOCTYPE_PUBLIC_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          this._err("missing-whitespace-after-doctype-public-keyword" /* missingWhitespaceAfterDoctypePublicKeyword */);
          token.publicId = "";
          this.state = 59 /* DOCTYPE_PUBLIC_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("missing-doctype-public-identifier" /* missingDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-public-identifier" /* missingQuoteBeforeDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // Before DOCTYPE public identifier state
    //------------------------------------------------------------------
    _stateBeforeDoctypePublicIdentifier(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          token.publicId = "";
          this.state = 58 /* DOCTYPE_PUBLIC_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          token.publicId = "";
          this.state = 59 /* DOCTYPE_PUBLIC_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("missing-doctype-public-identifier" /* missingDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-public-identifier" /* missingQuoteBeforeDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // DOCTYPE public identifier (double-quoted) state
    //------------------------------------------------------------------
    _stateDoctypePublicIdentifierDoubleQuoted(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 34 /* QUOTATION_MARK */: {
          this.state = 60 /* AFTER_DOCTYPE_PUBLIC_IDENTIFIER */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.publicId += REPLACEMENT_CHARACTER;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-doctype-public-identifier" /* abruptDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.publicId += String.fromCodePoint(cp);
        }
      }
    }
    // DOCTYPE public identifier (single-quoted) state
    //------------------------------------------------------------------
    _stateDoctypePublicIdentifierSingleQuoted(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 39 /* APOSTROPHE */: {
          this.state = 60 /* AFTER_DOCTYPE_PUBLIC_IDENTIFIER */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.publicId += REPLACEMENT_CHARACTER;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-doctype-public-identifier" /* abruptDoctypePublicIdentifier */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.publicId += String.fromCodePoint(cp);
        }
      }
    }
    // After DOCTYPE public identifier state
    //------------------------------------------------------------------
    _stateAfterDoctypePublicIdentifier(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 61 /* BETWEEN_DOCTYPE_PUBLIC_AND_SYSTEM_IDENTIFIERS */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          this._err("missing-whitespace-between-doctype-public-and-system-identifiers" /* missingWhitespaceBetweenDoctypePublicAndSystemIdentifiers */);
          token.systemId = "";
          this.state = 64 /* DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          this._err("missing-whitespace-between-doctype-public-and-system-identifiers" /* missingWhitespaceBetweenDoctypePublicAndSystemIdentifiers */);
          token.systemId = "";
          this.state = 65 /* DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-system-identifier" /* missingQuoteBeforeDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // Between DOCTYPE public and system identifiers state
    //------------------------------------------------------------------
    _stateBetweenDoctypePublicAndSystemIdentifiers(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          token.systemId = "";
          this.state = 64 /* DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          token.systemId = "";
          this.state = 65 /* DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-system-identifier" /* missingQuoteBeforeDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // After DOCTYPE system keyword state
    //------------------------------------------------------------------
    _stateAfterDoctypeSystemKeyword(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          this.state = 63 /* BEFORE_DOCTYPE_SYSTEM_IDENTIFIER */;
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          this._err("missing-whitespace-after-doctype-system-keyword" /* missingWhitespaceAfterDoctypeSystemKeyword */);
          token.systemId = "";
          this.state = 64 /* DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          this._err("missing-whitespace-after-doctype-system-keyword" /* missingWhitespaceAfterDoctypeSystemKeyword */);
          token.systemId = "";
          this.state = 65 /* DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("missing-doctype-system-identifier" /* missingDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-system-identifier" /* missingQuoteBeforeDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // Before DOCTYPE system identifier state
    //------------------------------------------------------------------
    _stateBeforeDoctypeSystemIdentifier(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 34 /* QUOTATION_MARK */: {
          token.systemId = "";
          this.state = 64 /* DOCTYPE_SYSTEM_IDENTIFIER_DOUBLE_QUOTED */;
          break;
        }
        case 39 /* APOSTROPHE */: {
          token.systemId = "";
          this.state = 65 /* DOCTYPE_SYSTEM_IDENTIFIER_SINGLE_QUOTED */;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("missing-doctype-system-identifier" /* missingDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 0 /* DATA */;
          this.emitCurrentDoctype(token);
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("missing-quote-before-doctype-system-identifier" /* missingQuoteBeforeDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // DOCTYPE system identifier (double-quoted) state
    //------------------------------------------------------------------
    _stateDoctypeSystemIdentifierDoubleQuoted(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 34 /* QUOTATION_MARK */: {
          this.state = 66 /* AFTER_DOCTYPE_SYSTEM_IDENTIFIER */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.systemId += REPLACEMENT_CHARACTER;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-doctype-system-identifier" /* abruptDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.systemId += String.fromCodePoint(cp);
        }
      }
    }
    // DOCTYPE system identifier (single-quoted) state
    //------------------------------------------------------------------
    _stateDoctypeSystemIdentifierSingleQuoted(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 39 /* APOSTROPHE */: {
          this.state = 66 /* AFTER_DOCTYPE_SYSTEM_IDENTIFIER */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          token.systemId += REPLACEMENT_CHARACTER;
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this._err("abrupt-doctype-system-identifier" /* abruptDoctypeSystemIdentifier */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          token.systemId += String.fromCodePoint(cp);
        }
      }
    }
    // After DOCTYPE system identifier state
    //------------------------------------------------------------------
    _stateAfterDoctypeSystemIdentifier(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 32 /* SPACE */:
        case 10 /* LINE_FEED */:
        case 9 /* TABULATION */:
        case 12 /* FORM_FEED */: {
          break;
        }
        case 62 /* GREATER_THAN_SIGN */: {
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-doctype" /* eofInDoctype */);
          token.forceQuirks = true;
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default: {
          this._err("unexpected-character-after-doctype-system-identifier" /* unexpectedCharacterAfterDoctypeSystemIdentifier */);
          this.state = 67 /* BOGUS_DOCTYPE */;
          this._stateBogusDoctype(cp);
        }
      }
    }
    // Bogus DOCTYPE state
    //------------------------------------------------------------------
    _stateBogusDoctype(cp) {
      const token = this.currentToken;
      switch (cp) {
        case 62 /* GREATER_THAN_SIGN */: {
          this.emitCurrentDoctype(token);
          this.state = 0 /* DATA */;
          break;
        }
        case 0 /* NULL */: {
          this._err("unexpected-null-character" /* unexpectedNullCharacter */);
          break;
        }
        case -1 /* EOF */: {
          this.emitCurrentDoctype(token);
          this._emitEOFToken();
          break;
        }
        default:
      }
    }
    // CDATA section state
    //------------------------------------------------------------------
    _stateCdataSection(cp) {
      switch (cp) {
        case 93 /* RIGHT_SQUARE_BRACKET */: {
          this.state = 69 /* CDATA_SECTION_BRACKET */;
          break;
        }
        case -1 /* EOF */: {
          this._err("eof-in-cdata" /* eofInCdata */);
          this._emitEOFToken();
          break;
        }
        default: {
          this._emitCodePoint(cp);
        }
      }
    }
    // CDATA section bracket state
    //------------------------------------------------------------------
    _stateCdataSectionBracket(cp) {
      if (cp === 93 /* RIGHT_SQUARE_BRACKET */) {
        this.state = 70 /* CDATA_SECTION_END */;
      } else {
        this._emitChars("]");
        this.state = 68 /* CDATA_SECTION */;
        this._stateCdataSection(cp);
      }
    }
    // CDATA section end state
    //------------------------------------------------------------------
    _stateCdataSectionEnd(cp) {
      switch (cp) {
        case 62 /* GREATER_THAN_SIGN */: {
          this.state = 0 /* DATA */;
          break;
        }
        case 93 /* RIGHT_SQUARE_BRACKET */: {
          this._emitChars("]");
          break;
        }
        default: {
          this._emitChars("]]");
          this.state = 68 /* CDATA_SECTION */;
          this._stateCdataSection(cp);
        }
      }
    }
    // Character reference state
    //------------------------------------------------------------------
    _stateCharacterReference() {
      let length = this.entityDecoder.write(this.preprocessor.html, this.preprocessor.pos);
      if (length < 0) {
        if (this.preprocessor.lastChunkWritten) {
          length = this.entityDecoder.end();
        } else {
          this.active = false;
          this.preprocessor.pos = this.preprocessor.html.length - 1;
          this.consumedAfterSnapshot = 0;
          this.preprocessor.endOfChunkHit = true;
          return;
        }
      }
      if (length === 0) {
        this.preprocessor.pos = this.entityStartPos;
        this._flushCodePointConsumedAsCharacterReference(38 /* AMPERSAND */);
        this.state = !this._isCharacterReferenceInAttribute() && isAsciiAlphaNumeric(this.preprocessor.peek(1)) ? 72 /* AMBIGUOUS_AMPERSAND */ : this.returnState;
      } else {
        this.state = this.returnState;
      }
    }
    // Ambiguos ampersand state
    //------------------------------------------------------------------
    _stateAmbiguousAmpersand(cp) {
      if (isAsciiAlphaNumeric(cp)) {
        this._flushCodePointConsumedAsCharacterReference(cp);
      } else {
        if (cp === 59 /* SEMICOLON */) {
          this._err("unknown-named-character-reference" /* unknownNamedCharacterReference */);
        }
        this.state = this.returnState;
        this._callState(cp);
      }
    }
  };

  // src/parser/open-element-stack.ts
  var IMPLICIT_END_TAG_REQUIRED = /* @__PURE__ */ new Set([22 /* DD */, 29 /* DT */, 59 /* LI */, 81 /* OPTGROUP */, 82 /* OPTION */, 83 /* P */, 87 /* RB */, 88 /* RP */, 89 /* RT */, 90 /* RTC */]);
  var IMPLICIT_END_TAG_REQUIRED_THOROUGHLY = /* @__PURE__ */ new Set([
    ...IMPLICIT_END_TAG_REQUIRED,
    17 /* CAPTION */,
    21 /* COLGROUP */,
    107 /* TBODY */,
    111 /* TD */,
    110 /* TFOOT */,
    112 /* TH */,
    113 /* THEAD */,
    115 /* TR */
  ]);
  var SCOPING_ELEMENTS_HTML = /* @__PURE__ */ new Set([
    4 /* APPLET */,
    17 /* CAPTION */,
    51 /* HTML */,
    64 /* MARQUEE */,
    79 /* OBJECT */,
    106 /* TABLE */,
    111 /* TD */,
    108 /* TEMPLATE */,
    112 /* TH */
  ]);
  var SCOPING_ELEMENTS_HTML_LIST = /* @__PURE__ */ new Set([...SCOPING_ELEMENTS_HTML, 80 /* OL */, 119 /* UL */]);
  var SCOPING_ELEMENTS_HTML_BUTTON = /* @__PURE__ */ new Set([...SCOPING_ELEMENTS_HTML, 16 /* BUTTON */]);
  var SCOPING_ELEMENTS_MATHML = /* @__PURE__ */ new Set([3 /* ANNOTATION_XML */, 69 /* MI */, 71 /* MN */, 70 /* MO */, 72 /* MS */, 73 /* MTEXT */]);
  var SCOPING_ELEMENTS_SVG = /* @__PURE__ */ new Set([23 /* DESC */, 37 /* FOREIGN_OBJECT */, 114 /* TITLE */]);
  var TABLE_ROW_CONTEXT = /* @__PURE__ */ new Set([115 /* TR */, 108 /* TEMPLATE */, 51 /* HTML */]);
  var TABLE_BODY_CONTEXT = /* @__PURE__ */ new Set([107 /* TBODY */, 110 /* TFOOT */, 113 /* THEAD */, 108 /* TEMPLATE */, 51 /* HTML */]);
  var TABLE_CONTEXT = /* @__PURE__ */ new Set([106 /* TABLE */, 108 /* TEMPLATE */, 51 /* HTML */]);
  var TABLE_CELLS = /* @__PURE__ */ new Set([111 /* TD */, 112 /* TH */]);
  var OpenElementStack = class {
    constructor(document, treeAdapter, handler) {
      this.treeAdapter = treeAdapter;
      this.handler = handler;
      __publicField(this, "items", []);
      __publicField(this, "tagIDs", []);
      __publicField(this, "current");
      __publicField(this, "stackTop", -1);
      __publicField(this, "tmplCount", 0);
      __publicField(this, "currentTagId", 0 /* UNKNOWN */);
      this.current = document;
    }
    get currentTmplContentOrNode() {
      return this._isInTemplate() ? this.treeAdapter.getTemplateContent(this.current) : this.current;
    }
    //Index of element
    _indexOf(element) {
      return this.items.lastIndexOf(element, this.stackTop);
    }
    //Update current element
    _isInTemplate() {
      return this.currentTagId === 108 /* TEMPLATE */ && this.treeAdapter.getNamespaceURI(this.current) === "http://www.w3.org/1999/xhtml" /* HTML */;
    }
    _updateCurrentElement() {
      this.current = this.items[this.stackTop];
      this.currentTagId = this.tagIDs[this.stackTop];
    }
    //Mutations
    push(element, tagID) {
      this.stackTop++;
      this.items[this.stackTop] = element;
      this.current = element;
      this.tagIDs[this.stackTop] = tagID;
      this.currentTagId = tagID;
      if (this._isInTemplate()) {
        this.tmplCount++;
      }
      this.handler.onItemPush(element, tagID, true);
    }
    pop() {
      const popped = this.current;
      if (this.tmplCount > 0 && this._isInTemplate()) {
        this.tmplCount--;
      }
      this.stackTop--;
      this._updateCurrentElement();
      this.handler.onItemPop(popped, true);
    }
    replace(oldElement, newElement) {
      const idx = this._indexOf(oldElement);
      this.items[idx] = newElement;
      if (idx === this.stackTop) {
        this.current = newElement;
      }
    }
    insertAfter(referenceElement, newElement, newElementID) {
      const insertionIdx = this._indexOf(referenceElement) + 1;
      this.items.splice(insertionIdx, 0, newElement);
      this.tagIDs.splice(insertionIdx, 0, newElementID);
      this.stackTop++;
      if (insertionIdx === this.stackTop) {
        this._updateCurrentElement();
      }
      if (this.current && this.currentTagId !== void 0) {
        this.handler.onItemPush(this.current, this.currentTagId, insertionIdx === this.stackTop);
      }
    }
    popUntilTagNamePopped(tagName) {
      let targetIdx = this.stackTop + 1;
      do {
        targetIdx = this.tagIDs.lastIndexOf(tagName, targetIdx - 1);
      } while (targetIdx > 0 && this.treeAdapter.getNamespaceURI(this.items[targetIdx]) !== "http://www.w3.org/1999/xhtml" /* HTML */);
      this.shortenToLength(Math.max(targetIdx, 0));
    }
    shortenToLength(idx) {
      while (this.stackTop >= idx) {
        const popped = this.current;
        if (this.tmplCount > 0 && this._isInTemplate()) {
          this.tmplCount -= 1;
        }
        this.stackTop--;
        this._updateCurrentElement();
        this.handler.onItemPop(popped, this.stackTop < idx);
      }
    }
    popUntilElementPopped(element) {
      const idx = this._indexOf(element);
      this.shortenToLength(Math.max(idx, 0));
    }
    popUntilPopped(tagNames, targetNS) {
      const idx = this._indexOfTagNames(tagNames, targetNS);
      this.shortenToLength(Math.max(idx, 0));
    }
    popUntilNumberedHeaderPopped() {
      this.popUntilPopped(NUMBERED_HEADERS, "http://www.w3.org/1999/xhtml" /* HTML */);
    }
    popUntilTableCellPopped() {
      this.popUntilPopped(TABLE_CELLS, "http://www.w3.org/1999/xhtml" /* HTML */);
    }
    popAllUpToHtmlElement() {
      this.tmplCount = 0;
      this.shortenToLength(1);
    }
    _indexOfTagNames(tagNames, namespace) {
      for (let i = this.stackTop; i >= 0; i--) {
        if (tagNames.has(this.tagIDs[i]) && this.treeAdapter.getNamespaceURI(this.items[i]) === namespace) {
          return i;
        }
      }
      return -1;
    }
    clearBackTo(tagNames, targetNS) {
      const idx = this._indexOfTagNames(tagNames, targetNS);
      this.shortenToLength(idx + 1);
    }
    clearBackToTableContext() {
      this.clearBackTo(TABLE_CONTEXT, "http://www.w3.org/1999/xhtml" /* HTML */);
    }
    clearBackToTableBodyContext() {
      this.clearBackTo(TABLE_BODY_CONTEXT, "http://www.w3.org/1999/xhtml" /* HTML */);
    }
    clearBackToTableRowContext() {
      this.clearBackTo(TABLE_ROW_CONTEXT, "http://www.w3.org/1999/xhtml" /* HTML */);
    }
    remove(element) {
      const idx = this._indexOf(element);
      if (idx >= 0) {
        if (idx === this.stackTop) {
          this.pop();
        } else {
          this.items.splice(idx, 1);
          this.tagIDs.splice(idx, 1);
          this.stackTop--;
          this._updateCurrentElement();
          this.handler.onItemPop(element, false);
        }
      }
    }
    //Search
    tryPeekProperlyNestedBodyElement() {
      return this.stackTop >= 1 && this.tagIDs[1] === 14 /* BODY */ ? this.items[1] : null;
    }
    contains(element) {
      return this._indexOf(element) > -1;
    }
    getCommonAncestor(element) {
      const elementIdx = this._indexOf(element) - 1;
      return elementIdx >= 0 ? this.items[elementIdx] : null;
    }
    isRootHtmlElementCurrent() {
      return this.stackTop === 0 && this.tagIDs[0] === 51 /* HTML */;
    }
    //Element in scope
    hasInDynamicScope(tagName, htmlScope) {
      for (let i = this.stackTop; i >= 0; i--) {
        const tn = this.tagIDs[i];
        switch (this.treeAdapter.getNamespaceURI(this.items[i])) {
          case "http://www.w3.org/1999/xhtml" /* HTML */: {
            if (tn === tagName) return true;
            if (htmlScope.has(tn)) return false;
            break;
          }
          case "http://www.w3.org/2000/svg" /* SVG */: {
            if (SCOPING_ELEMENTS_SVG.has(tn)) return false;
            break;
          }
          case "http://www.w3.org/1998/Math/MathML" /* MATHML */: {
            if (SCOPING_ELEMENTS_MATHML.has(tn)) return false;
            break;
          }
        }
      }
      return true;
    }
    hasInScope(tagName) {
      return this.hasInDynamicScope(tagName, SCOPING_ELEMENTS_HTML);
    }
    hasInListItemScope(tagName) {
      return this.hasInDynamicScope(tagName, SCOPING_ELEMENTS_HTML_LIST);
    }
    hasInButtonScope(tagName) {
      return this.hasInDynamicScope(tagName, SCOPING_ELEMENTS_HTML_BUTTON);
    }
    hasNumberedHeaderInScope() {
      for (let i = this.stackTop; i >= 0; i--) {
        const tn = this.tagIDs[i];
        switch (this.treeAdapter.getNamespaceURI(this.items[i])) {
          case "http://www.w3.org/1999/xhtml" /* HTML */: {
            if (NUMBERED_HEADERS.has(tn)) return true;
            if (SCOPING_ELEMENTS_HTML.has(tn)) return false;
            break;
          }
          case "http://www.w3.org/2000/svg" /* SVG */: {
            if (SCOPING_ELEMENTS_SVG.has(tn)) return false;
            break;
          }
          case "http://www.w3.org/1998/Math/MathML" /* MATHML */: {
            if (SCOPING_ELEMENTS_MATHML.has(tn)) return false;
            break;
          }
        }
      }
      return true;
    }
    hasInTableScope(tagName) {
      for (let i = this.stackTop; i >= 0; i--) {
        if (this.treeAdapter.getNamespaceURI(this.items[i]) !== "http://www.w3.org/1999/xhtml" /* HTML */) {
          continue;
        }
        switch (this.tagIDs[i]) {
          case tagName: {
            return true;
          }
          case 106 /* TABLE */:
          case 51 /* HTML */: {
            return false;
          }
        }
      }
      return true;
    }
    hasTableBodyContextInTableScope() {
      for (let i = this.stackTop; i >= 0; i--) {
        if (this.treeAdapter.getNamespaceURI(this.items[i]) !== "http://www.w3.org/1999/xhtml" /* HTML */) {
          continue;
        }
        switch (this.tagIDs[i]) {
          case 107 /* TBODY */:
          case 113 /* THEAD */:
          case 110 /* TFOOT */: {
            return true;
          }
          case 106 /* TABLE */:
          case 51 /* HTML */: {
            return false;
          }
        }
      }
      return true;
    }
    hasInSelectScope(tagName) {
      for (let i = this.stackTop; i >= 0; i--) {
        if (this.treeAdapter.getNamespaceURI(this.items[i]) !== "http://www.w3.org/1999/xhtml" /* HTML */) {
          continue;
        }
        switch (this.tagIDs[i]) {
          case tagName: {
            return true;
          }
          case 82 /* OPTION */:
          case 81 /* OPTGROUP */: {
            break;
          }
          default: {
            return false;
          }
        }
      }
      return true;
    }
    //Implied end tags
    generateImpliedEndTags() {
      while (this.currentTagId !== void 0 && IMPLICIT_END_TAG_REQUIRED.has(this.currentTagId)) {
        this.pop();
      }
    }
    generateImpliedEndTagsThoroughly() {
      while (this.currentTagId !== void 0 && IMPLICIT_END_TAG_REQUIRED_THOROUGHLY.has(this.currentTagId)) {
        this.pop();
      }
    }
    generateImpliedEndTagsWithExclusion(exclusionId) {
      while (this.currentTagId !== void 0 && this.currentTagId !== exclusionId && IMPLICIT_END_TAG_REQUIRED_THOROUGHLY.has(this.currentTagId)) {
        this.pop();
      }
    }
  };

  // src/parser/formatting-element-list.ts
  var NOAH_ARK_CAPACITY = 3;
  var MARKER = { type: 0 /* Marker */ };
  var FormattingElementList = class {
    constructor(treeAdapter) {
      this.treeAdapter = treeAdapter;
      __publicField(this, "entries", []);
      __publicField(this, "bookmark", null);
    }
    //Noah Ark's condition
    //OPTIMIZATION: at first we try to find possible candidates for exclusion using
    //lightweight heuristics without thorough attributes check.
    _getNoahArkConditionCandidates(newElement, neAttrs) {
      const candidates = [];
      const neAttrsLength = neAttrs.length;
      const neTagName = this.treeAdapter.getTagName(newElement);
      const neNamespaceURI = this.treeAdapter.getNamespaceURI(newElement);
      for (let i = 0; i < this.entries.length; i++) {
        const entry = this.entries[i];
        if (entry.type === 0 /* Marker */) {
          break;
        }
        const { element } = entry;
        if (this.treeAdapter.getTagName(element) === neTagName && this.treeAdapter.getNamespaceURI(element) === neNamespaceURI) {
          const elementAttrs = this.treeAdapter.getAttrList(element);
          if (elementAttrs.length === neAttrsLength) {
            candidates.push({ idx: i, attrs: elementAttrs });
          }
        }
      }
      return candidates;
    }
    _ensureNoahArkCondition(newElement) {
      if (this.entries.length < NOAH_ARK_CAPACITY) return;
      const neAttrs = this.treeAdapter.getAttrList(newElement);
      const candidates = this._getNoahArkConditionCandidates(newElement, neAttrs);
      if (candidates.length < NOAH_ARK_CAPACITY) return;
      const neAttrsMap = new Map(neAttrs.map((neAttr) => [neAttr.name, neAttr.value]));
      let validCandidates = 0;
      for (let i = 0; i < candidates.length; i++) {
        const candidate = candidates[i];
        if (candidate.attrs.every((cAttr) => neAttrsMap.get(cAttr.name) === cAttr.value)) {
          validCandidates += 1;
          if (validCandidates >= NOAH_ARK_CAPACITY) {
            this.entries.splice(candidate.idx, 1);
          }
        }
      }
    }
    //Mutations
    insertMarker() {
      this.entries.unshift(MARKER);
    }
    pushElement(element, token) {
      this._ensureNoahArkCondition(element);
      this.entries.unshift({
        type: 1 /* Element */,
        element,
        token
      });
    }
    insertElementAfterBookmark(element, token) {
      const bookmarkIdx = this.entries.indexOf(this.bookmark);
      this.entries.splice(bookmarkIdx, 0, {
        type: 1 /* Element */,
        element,
        token
      });
    }
    removeEntry(entry) {
      const entryIndex = this.entries.indexOf(entry);
      if (entryIndex !== -1) {
        this.entries.splice(entryIndex, 1);
      }
    }
    /**
     * Clears the list of formatting elements up to the last marker.
     *
     * @see https://html.spec.whatwg.org/multipage/parsing.html#clear-the-list-of-active-formatting-elements-up-to-the-last-marker
     */
    clearToLastMarker() {
      const markerIdx = this.entries.indexOf(MARKER);
      if (markerIdx === -1) {
        this.entries.length = 0;
      } else {
        this.entries.splice(0, markerIdx + 1);
      }
    }
    //Search
    getElementEntryInScopeWithTagName(tagName) {
      const entry = this.entries.find(
        (entry2) => entry2.type === 0 /* Marker */ || this.treeAdapter.getTagName(entry2.element) === tagName
      );
      return entry && entry.type === 1 /* Element */ ? entry : null;
    }
    getElementEntry(element) {
      return this.entries.find(
        (entry) => entry.type === 1 /* Element */ && entry.element === element
      );
    }
  };

  // src/tree-adapters/default.ts
  var defaultTreeAdapter = {
    //Node construction
    createDocument() {
      return {
        nodeName: "#document",
        mode: "no-quirks" /* NO_QUIRKS */,
        childNodes: []
      };
    },
    createDocumentFragment() {
      return {
        nodeName: "#document-fragment",
        childNodes: []
      };
    },
    createElement(tagName, namespaceURI, attrs) {
      return {
        nodeName: tagName,
        tagName,
        attrs,
        namespaceURI,
        childNodes: [],
        parentNode: null
      };
    },
    createCommentNode(data) {
      return {
        nodeName: "#comment",
        data,
        parentNode: null
      };
    },
    createTextNode(value) {
      return {
        nodeName: "#text",
        value,
        parentNode: null
      };
    },
    //Tree mutation
    appendChild(parentNode, newNode) {
      parentNode.childNodes.push(newNode);
      newNode.parentNode = parentNode;
    },
    insertBefore(parentNode, newNode, referenceNode) {
      const insertionIdx = parentNode.childNodes.indexOf(referenceNode);
      parentNode.childNodes.splice(insertionIdx, 0, newNode);
      newNode.parentNode = parentNode;
    },
    setTemplateContent(templateElement, contentElement) {
      templateElement.content = contentElement;
    },
    getTemplateContent(templateElement) {
      return templateElement.content;
    },
    setDocumentType(document, name, publicId, systemId) {
      const doctypeNode = document.childNodes.find((node) => node.nodeName === "#documentType");
      if (doctypeNode) {
        doctypeNode.name = name;
        doctypeNode.publicId = publicId;
        doctypeNode.systemId = systemId;
      } else {
        const node = {
          nodeName: "#documentType",
          name,
          publicId,
          systemId,
          parentNode: null
        };
        defaultTreeAdapter.appendChild(document, node);
      }
    },
    setDocumentMode(document, mode) {
      document.mode = mode;
    },
    getDocumentMode(document) {
      return document.mode;
    },
    detachNode(node) {
      if (node.parentNode) {
        const idx = node.parentNode.childNodes.indexOf(node);
        node.parentNode.childNodes.splice(idx, 1);
        node.parentNode = null;
      }
    },
    insertText(parentNode, text) {
      if (parentNode.childNodes.length > 0) {
        const prevNode = parentNode.childNodes[parentNode.childNodes.length - 1];
        if (defaultTreeAdapter.isTextNode(prevNode)) {
          prevNode.value += text;
          return;
        }
      }
      defaultTreeAdapter.appendChild(parentNode, defaultTreeAdapter.createTextNode(text));
    },
    insertTextBefore(parentNode, text, referenceNode) {
      const prevNode = parentNode.childNodes[parentNode.childNodes.indexOf(referenceNode) - 1];
      if (prevNode && defaultTreeAdapter.isTextNode(prevNode)) {
        prevNode.value += text;
      } else {
        defaultTreeAdapter.insertBefore(parentNode, defaultTreeAdapter.createTextNode(text), referenceNode);
      }
    },
    adoptAttributes(recipient, attrs) {
      const recipientAttrsMap = new Set(recipient.attrs.map((attr) => attr.name));
      for (let j = 0; j < attrs.length; j++) {
        if (!recipientAttrsMap.has(attrs[j].name)) {
          recipient.attrs.push(attrs[j]);
        }
      }
    },
    //Tree traversing
    getFirstChild(node) {
      return node.childNodes[0];
    },
    getChildNodes(node) {
      return node.childNodes;
    },
    getParentNode(node) {
      return node.parentNode;
    },
    getAttrList(element) {
      return element.attrs;
    },
    //Node data
    getTagName(element) {
      return element.tagName;
    },
    getNamespaceURI(element) {
      return element.namespaceURI;
    },
    getTextNodeContent(textNode) {
      return textNode.value;
    },
    getCommentNodeContent(commentNode) {
      return commentNode.data;
    },
    getDocumentTypeNodeName(doctypeNode) {
      return doctypeNode.name;
    },
    getDocumentTypeNodePublicId(doctypeNode) {
      return doctypeNode.publicId;
    },
    getDocumentTypeNodeSystemId(doctypeNode) {
      return doctypeNode.systemId;
    },
    //Node types
    isTextNode(node) {
      return node.nodeName === "#text";
    },
    isCommentNode(node) {
      return node.nodeName === "#comment";
    },
    isDocumentTypeNode(node) {
      return node.nodeName === "#documentType";
    },
    isElementNode(node) {
      return Object.prototype.hasOwnProperty.call(node, "tagName");
    },
    // Source code location
    setNodeSourceCodeLocation(node, location) {
      node.sourceCodeLocation = location;
    },
    getNodeSourceCodeLocation(node) {
      return node.sourceCodeLocation;
    },
    updateNodeSourceCodeLocation(node, endLocation) {
      node.sourceCodeLocation = { ...node.sourceCodeLocation, ...endLocation };
    }
  };

  // src/common/doctype.ts
  var VALID_DOCTYPE_NAME = "html";
  var VALID_SYSTEM_ID = "about:legacy-compat";
  var QUIRKS_MODE_SYSTEM_ID = "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd";
  var QUIRKS_MODE_PUBLIC_ID_PREFIXES = [
    "+//silmaril//dtd html pro v0r11 19970101//",
    "-//as//dtd html 3.0 aswedit + extensions//",
    "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
    "-//ietf//dtd html 2.0 level 1//",
    "-//ietf//dtd html 2.0 level 2//",
    "-//ietf//dtd html 2.0 strict level 1//",
    "-//ietf//dtd html 2.0 strict level 2//",
    "-//ietf//dtd html 2.0 strict//",
    "-//ietf//dtd html 2.0//",
    "-//ietf//dtd html 2.1e//",
    "-//ietf//dtd html 3.0//",
    "-//ietf//dtd html 3.2 final//",
    "-//ietf//dtd html 3.2//",
    "-//ietf//dtd html 3//",
    "-//ietf//dtd html level 0//",
    "-//ietf//dtd html level 1//",
    "-//ietf//dtd html level 2//",
    "-//ietf//dtd html level 3//",
    "-//ietf//dtd html strict level 0//",
    "-//ietf//dtd html strict level 1//",
    "-//ietf//dtd html strict level 2//",
    "-//ietf//dtd html strict level 3//",
    "-//ietf//dtd html strict//",
    "-//ietf//dtd html//",
    "-//metrius//dtd metrius presentational//",
    "-//microsoft//dtd internet explorer 2.0 html strict//",
    "-//microsoft//dtd internet explorer 2.0 html//",
    "-//microsoft//dtd internet explorer 2.0 tables//",
    "-//microsoft//dtd internet explorer 3.0 html strict//",
    "-//microsoft//dtd internet explorer 3.0 html//",
    "-//microsoft//dtd internet explorer 3.0 tables//",
    "-//netscape comm. corp.//dtd html//",
    "-//netscape comm. corp.//dtd strict html//",
    "-//o'reilly and associates//dtd html 2.0//",
    "-//o'reilly and associates//dtd html extended 1.0//",
    "-//o'reilly and associates//dtd html extended relaxed 1.0//",
    "-//sq//dtd html 2.0 hotmetal + extensions//",
    "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
    "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
    "-//spyglass//dtd html 2.0 extended//",
    "-//sun microsystems corp.//dtd hotjava html//",
    "-//sun microsystems corp.//dtd hotjava strict html//",
    "-//w3c//dtd html 3 1995-03-24//",
    "-//w3c//dtd html 3.2 draft//",
    "-//w3c//dtd html 3.2 final//",
    "-//w3c//dtd html 3.2//",
    "-//w3c//dtd html 3.2s draft//",
    "-//w3c//dtd html 4.0 frameset//",
    "-//w3c//dtd html 4.0 transitional//",
    "-//w3c//dtd html experimental 19960712//",
    "-//w3c//dtd html experimental 970421//",
    "-//w3c//dtd w3 html//",
    "-//w3o//dtd w3 html 3.0//",
    "-//webtechs//dtd mozilla html 2.0//",
    "-//webtechs//dtd mozilla html//"
  ];
  var QUIRKS_MODE_NO_SYSTEM_ID_PUBLIC_ID_PREFIXES = [
    ...QUIRKS_MODE_PUBLIC_ID_PREFIXES,
    "-//w3c//dtd html 4.01 frameset//",
    "-//w3c//dtd html 4.01 transitional//"
  ];
  var QUIRKS_MODE_PUBLIC_IDS = /* @__PURE__ */ new Set([
    "-//w3o//dtd w3 html strict 3.0//en//",
    "-/w3c/dtd html 4.0 transitional/en",
    "html"
  ]);
  var LIMITED_QUIRKS_PUBLIC_ID_PREFIXES = ["-//w3c//dtd xhtml 1.0 frameset//", "-//w3c//dtd xhtml 1.0 transitional//"];
  var LIMITED_QUIRKS_WITH_SYSTEM_ID_PUBLIC_ID_PREFIXES = [
    ...LIMITED_QUIRKS_PUBLIC_ID_PREFIXES,
    "-//w3c//dtd html 4.01 frameset//",
    "-//w3c//dtd html 4.01 transitional//"
  ];
  function hasPrefix(publicId, prefixes) {
    return prefixes.some((prefix) => publicId.startsWith(prefix));
  }
  function isConforming(token) {
    return token.name === VALID_DOCTYPE_NAME && token.publicId === null && (token.systemId === null || token.systemId === VALID_SYSTEM_ID);
  }
  function getDocumentMode(token) {
    if (token.name !== VALID_DOCTYPE_NAME) {
      return "quirks" /* QUIRKS */;
    }
    const { systemId } = token;
    if (systemId && systemId.toLowerCase() === QUIRKS_MODE_SYSTEM_ID) {
      return "quirks" /* QUIRKS */;
    }
    let { publicId } = token;
    if (publicId !== null) {
      publicId = publicId.toLowerCase();
      if (QUIRKS_MODE_PUBLIC_IDS.has(publicId)) {
        return "quirks" /* QUIRKS */;
      }
      let prefixes = systemId === null ? QUIRKS_MODE_NO_SYSTEM_ID_PUBLIC_ID_PREFIXES : QUIRKS_MODE_PUBLIC_ID_PREFIXES;
      if (hasPrefix(publicId, prefixes)) {
        return "quirks" /* QUIRKS */;
      }
      prefixes = systemId === null ? LIMITED_QUIRKS_PUBLIC_ID_PREFIXES : LIMITED_QUIRKS_WITH_SYSTEM_ID_PUBLIC_ID_PREFIXES;
      if (hasPrefix(publicId, prefixes)) {
        return "limited-quirks" /* LIMITED_QUIRKS */;
      }
    }
    return "no-quirks" /* NO_QUIRKS */;
  }

  // src/common/foreign-content.ts
  var foreign_content_exports = {};
  __export(foreign_content_exports, {
    SVG_TAG_NAMES_ADJUSTMENT_MAP: () => SVG_TAG_NAMES_ADJUSTMENT_MAP,
    adjustTokenMathMLAttrs: () => adjustTokenMathMLAttrs,
    adjustTokenSVGAttrs: () => adjustTokenSVGAttrs,
    adjustTokenSVGTagName: () => adjustTokenSVGTagName,
    adjustTokenXMLAttrs: () => adjustTokenXMLAttrs,
    causesExit: () => causesExit,
    isIntegrationPoint: () => isIntegrationPoint
  });
  var MIME_TYPES = {
    TEXT_HTML: "text/html",
    APPLICATION_XML: "application/xhtml+xml"
  };
  var DEFINITION_URL_ATTR = "definitionurl";
  var ADJUSTED_DEFINITION_URL_ATTR = "definitionURL";
  var SVG_ATTRS_ADJUSTMENT_MAP = new Map(
    [
      "attributeName",
      "attributeType",
      "baseFrequency",
      "baseProfile",
      "calcMode",
      "clipPathUnits",
      "diffuseConstant",
      "edgeMode",
      "filterUnits",
      "glyphRef",
      "gradientTransform",
      "gradientUnits",
      "kernelMatrix",
      "kernelUnitLength",
      "keyPoints",
      "keySplines",
      "keyTimes",
      "lengthAdjust",
      "limitingConeAngle",
      "markerHeight",
      "markerUnits",
      "markerWidth",
      "maskContentUnits",
      "maskUnits",
      "numOctaves",
      "pathLength",
      "patternContentUnits",
      "patternTransform",
      "patternUnits",
      "pointsAtX",
      "pointsAtY",
      "pointsAtZ",
      "preserveAlpha",
      "preserveAspectRatio",
      "primitiveUnits",
      "refX",
      "refY",
      "repeatCount",
      "repeatDur",
      "requiredExtensions",
      "requiredFeatures",
      "specularConstant",
      "specularExponent",
      "spreadMethod",
      "startOffset",
      "stdDeviation",
      "stitchTiles",
      "surfaceScale",
      "systemLanguage",
      "tableValues",
      "targetX",
      "targetY",
      "textLength",
      "viewBox",
      "viewTarget",
      "xChannelSelector",
      "yChannelSelector",
      "zoomAndPan"
    ].map((attr) => [attr.toLowerCase(), attr])
  );
  var XML_ATTRS_ADJUSTMENT_MAP = /* @__PURE__ */ new Map([
    ["xlink:actuate", { prefix: "xlink", name: "actuate", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:arcrole", { prefix: "xlink", name: "arcrole", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:href", { prefix: "xlink", name: "href", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:role", { prefix: "xlink", name: "role", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:show", { prefix: "xlink", name: "show", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:title", { prefix: "xlink", name: "title", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xlink:type", { prefix: "xlink", name: "type", namespace: "http://www.w3.org/1999/xlink" /* XLINK */ }],
    ["xml:lang", { prefix: "xml", name: "lang", namespace: "http://www.w3.org/XML/1998/namespace" /* XML */ }],
    ["xml:space", { prefix: "xml", name: "space", namespace: "http://www.w3.org/XML/1998/namespace" /* XML */ }],
    ["xmlns", { prefix: "", name: "xmlns", namespace: "http://www.w3.org/2000/xmlns/" /* XMLNS */ }],
    ["xmlns:xlink", { prefix: "xmlns", name: "xlink", namespace: "http://www.w3.org/2000/xmlns/" /* XMLNS */ }]
  ]);
  var SVG_TAG_NAMES_ADJUSTMENT_MAP = new Map(
    [
      "altGlyph",
      "altGlyphDef",
      "altGlyphItem",
      "animateColor",
      "animateMotion",
      "animateTransform",
      "clipPath",
      "feBlend",
      "feColorMatrix",
      "feComponentTransfer",
      "feComposite",
      "feConvolveMatrix",
      "feDiffuseLighting",
      "feDisplacementMap",
      "feDistantLight",
      "feFlood",
      "feFuncA",
      "feFuncB",
      "feFuncG",
      "feFuncR",
      "feGaussianBlur",
      "feImage",
      "feMerge",
      "feMergeNode",
      "feMorphology",
      "feOffset",
      "fePointLight",
      "feSpecularLighting",
      "feSpotLight",
      "feTile",
      "feTurbulence",
      "foreignObject",
      "glyphRef",
      "linearGradient",
      "radialGradient",
      "textPath"
    ].map((tn) => [tn.toLowerCase(), tn])
  );
  var EXITS_FOREIGN_CONTENT = /* @__PURE__ */ new Set([
    8 /* B */,
    12 /* BIG */,
    13 /* BLOCKQUOTE */,
    14 /* BODY */,
    15 /* BR */,
    18 /* CENTER */,
    19 /* CODE */,
    22 /* DD */,
    27 /* DIV */,
    28 /* DL */,
    29 /* DT */,
    30 /* EM */,
    31 /* EMBED */,
    41 /* H1 */,
    42 /* H2 */,
    43 /* H3 */,
    44 /* H4 */,
    45 /* H5 */,
    46 /* H6 */,
    47 /* HEAD */,
    50 /* HR */,
    52 /* I */,
    53 /* IMG */,
    59 /* LI */,
    61 /* LISTING */,
    66 /* MENU */,
    67 /* META */,
    75 /* NOBR */,
    80 /* OL */,
    83 /* P */,
    86 /* PRE */,
    91 /* RUBY */,
    92 /* S */,
    98 /* SMALL */,
    99 /* SPAN */,
    101 /* STRONG */,
    100 /* STRIKE */,
    103 /* SUB */,
    105 /* SUP */,
    106 /* TABLE */,
    117 /* TT */,
    118 /* U */,
    119 /* UL */,
    121 /* VAR */
  ]);
  function causesExit(startTagToken) {
    const tn = startTagToken.tagID;
    const isFontWithAttrs = tn === 35 /* FONT */ && startTagToken.attrs.some(({ name }) => name === "color" /* COLOR */ || name === "size" /* SIZE */ || name === "face" /* FACE */);
    return isFontWithAttrs || EXITS_FOREIGN_CONTENT.has(tn);
  }
  function adjustTokenMathMLAttrs(token) {
    for (let i = 0; i < token.attrs.length; i++) {
      if (token.attrs[i].name === DEFINITION_URL_ATTR) {
        token.attrs[i].name = ADJUSTED_DEFINITION_URL_ATTR;
        break;
      }
    }
  }
  function adjustTokenSVGAttrs(token) {
    for (let i = 0; i < token.attrs.length; i++) {
      const adjustedAttrName = SVG_ATTRS_ADJUSTMENT_MAP.get(token.attrs[i].name);
      if (adjustedAttrName != null) {
        token.attrs[i].name = adjustedAttrName;
      }
    }
  }
  function adjustTokenXMLAttrs(token) {
    for (let i = 0; i < token.attrs.length; i++) {
      const adjustedAttrEntry = XML_ATTRS_ADJUSTMENT_MAP.get(token.attrs[i].name);
      if (adjustedAttrEntry) {
        token.attrs[i].prefix = adjustedAttrEntry.prefix;
        token.attrs[i].name = adjustedAttrEntry.name;
        token.attrs[i].namespace = adjustedAttrEntry.namespace;
      }
    }
  }
  function adjustTokenSVGTagName(token) {
    const adjustedTagName = SVG_TAG_NAMES_ADJUSTMENT_MAP.get(token.tagName);
    if (adjustedTagName != null) {
      token.tagName = adjustedTagName;
      token.tagID = getTagID(token.tagName);
    }
  }
  function isMathMLTextIntegrationPoint(tn, ns) {
    return ns === "http://www.w3.org/1998/Math/MathML" /* MATHML */ && (tn === 69 /* MI */ || tn === 70 /* MO */ || tn === 71 /* MN */ || tn === 72 /* MS */ || tn === 73 /* MTEXT */);
  }
  function isHtmlIntegrationPoint(tn, ns, attrs) {
    if (ns === "http://www.w3.org/1998/Math/MathML" /* MATHML */ && tn === 3 /* ANNOTATION_XML */) {
      for (let i = 0; i < attrs.length; i++) {
        if (attrs[i].name === "encoding" /* ENCODING */) {
          const value = attrs[i].value.toLowerCase();
          return value === MIME_TYPES.TEXT_HTML || value === MIME_TYPES.APPLICATION_XML;
        }
      }
    }
    return ns === "http://www.w3.org/2000/svg" /* SVG */ && (tn === 37 /* FOREIGN_OBJECT */ || tn === 23 /* DESC */ || tn === 114 /* TITLE */);
  }
  function isIntegrationPoint(tn, ns, attrs, foreignNS) {
    return (!foreignNS || foreignNS === "http://www.w3.org/1999/xhtml" /* HTML */) && isHtmlIntegrationPoint(tn, ns, attrs) || (!foreignNS || foreignNS === "http://www.w3.org/1998/Math/MathML" /* MATHML */) && isMathMLTextIntegrationPoint(tn, ns);
  }

  // src/parser/index.ts
  var HIDDEN_INPUT_TYPE = "hidden";
  var AA_OUTER_LOOP_ITER = 8;
  var AA_INNER_LOOP_ITER = 3;
  var BASE_LOC = {
    startLine: -1,
    startCol: -1,
    startOffset: -1,
    endLine: -1,
    endCol: -1,
    endOffset: -1
  };
  var TABLE_STRUCTURE_TAGS = /* @__PURE__ */ new Set([106 /* TABLE */, 107 /* TBODY */, 110 /* TFOOT */, 113 /* THEAD */, 115 /* TR */]);
  var defaultParserOptions = {
    scriptingEnabled: true,
    sourceCodeLocationInfo: false,
    treeAdapter: defaultTreeAdapter,
    onParseError: null
  };
  var Parser = class {
    constructor(options, document, fragmentContext = null, scriptHandler = null) {
      this.fragmentContext = fragmentContext;
      this.scriptHandler = scriptHandler;
      __publicField(this, "treeAdapter");
      /** @internal */
      __publicField(this, "onParseError");
      __publicField(this, "currentToken", null);
      __publicField(this, "options");
      __publicField(this, "document");
      __publicField(this, "tokenizer");
      __publicField(this, "stopped", false);
      /** @internal */
      __publicField(this, "insertionMode", 0 /* INITIAL */);
      /** @internal */
      __publicField(this, "originalInsertionMode", 0 /* INITIAL */);
      /** @internal */
      __publicField(this, "fragmentContextID");
      /** @internal */
      __publicField(this, "headElement", null);
      /** @internal */
      __publicField(this, "formElement", null);
      /** @internal */
      __publicField(this, "openElements");
      /** @internal */
      __publicField(this, "activeFormattingElements");
      /** Indicates that the current node is not an element in the HTML namespace */
      __publicField(this, "currentNotInHTML", false);
      /**
       * The template insertion mode stack is maintained from the left.
       * Ie. the topmost element will always have index 0.
       *
       * @internal
       */
      __publicField(this, "tmplInsertionModeStack", []);
      /** @internal */
      __publicField(this, "pendingCharacterTokens", []);
      /** @internal */
      __publicField(this, "hasNonWhitespacePendingCharacterToken", false);
      /** @internal */
      __publicField(this, "framesetOk", true);
      /** @internal */
      __publicField(this, "skipNextNewLine", false);
      /** @internal */
      __publicField(this, "fosterParentingEnabled", false);
      this.options = {
        ...defaultParserOptions,
        ...options
      };
      this.treeAdapter = this.options.treeAdapter;
      this.onParseError = this.options.onParseError;
      if (this.onParseError) {
        this.options.sourceCodeLocationInfo = true;
      }
      this.document = document ?? this.treeAdapter.createDocument();
      this.tokenizer = new Tokenizer(this.options, this);
      this.activeFormattingElements = new FormattingElementList(this.treeAdapter);
      this.fragmentContextID = fragmentContext ? getTagID(this.treeAdapter.getTagName(fragmentContext)) : 0 /* UNKNOWN */;
      this._setContextModes(fragmentContext ?? this.document, this.fragmentContextID);
      this.openElements = new OpenElementStack(this.document, this.treeAdapter, this);
    }
    // API
    static parse(html, options) {
      const parser = new this(options);
      parser.tokenizer.write(html, true);
      return parser.document;
    }
    static getFragmentParser(fragmentContext, options) {
      const opts = {
        ...defaultParserOptions,
        ...options
      };
      fragmentContext ?? (fragmentContext = opts.treeAdapter.createElement("template" /* TEMPLATE */, "http://www.w3.org/1999/xhtml" /* HTML */, []));
      const documentMock = opts.treeAdapter.createElement("documentmock", "http://www.w3.org/1999/xhtml" /* HTML */, []);
      const parser = new this(opts, documentMock, fragmentContext);
      if (parser.fragmentContextID === 108 /* TEMPLATE */) {
        parser.tmplInsertionModeStack.unshift(17 /* IN_TEMPLATE */);
      }
      parser._initTokenizerForFragmentParsing();
      parser._insertFakeRootElement();
      parser._resetInsertionMode();
      parser._findFormInFragmentContext();
      return parser;
    }
    getFragment() {
      const rootElement = this.treeAdapter.getFirstChild(this.document);
      const fragment = this.treeAdapter.createDocumentFragment();
      this._adoptNodes(rootElement, fragment);
      return fragment;
    }
    //Errors
    /** @internal */
    _err(token, code, beforeToken) {
      if (!this.onParseError) return;
      const loc = token.location ?? BASE_LOC;
      const err = {
        code,
        startLine: loc.startLine,
        startCol: loc.startCol,
        startOffset: loc.startOffset,
        endLine: beforeToken ? loc.startLine : loc.endLine,
        endCol: beforeToken ? loc.startCol : loc.endCol,
        endOffset: beforeToken ? loc.startOffset : loc.endOffset
      };
      this.onParseError(err);
    }
    //Stack events
    /** @internal */
    onItemPush(node, tid, isTop) {
      this.treeAdapter.onItemPush?.(node);
      if (isTop && this.openElements.stackTop > 0) this._setContextModes(node, tid);
    }
    /** @internal */
    onItemPop(node, isTop) {
      if (this.options.sourceCodeLocationInfo) {
        this._setEndLocation(node, this.currentToken);
      }
      this.treeAdapter.onItemPop?.(node, this.openElements.current);
      if (isTop) {
        let current;
        let currentTagId;
        if (this.openElements.stackTop === 0 && this.fragmentContext) {
          current = this.fragmentContext;
          currentTagId = this.fragmentContextID;
        } else {
          ({ current, currentTagId } = this.openElements);
        }
        this._setContextModes(current, currentTagId);
      }
    }
    _setContextModes(current, tid) {
      const isHTML = current === this.document || current && this.treeAdapter.getNamespaceURI(current) === "http://www.w3.org/1999/xhtml" /* HTML */;
      this.currentNotInHTML = !isHTML;
      this.tokenizer.inForeignNode = !isHTML && current !== void 0 && tid !== void 0 && !this._isIntegrationPoint(tid, current);
    }
    /** @protected */
    _switchToTextParsing(currentToken, nextTokenizerState) {
      this._insertElement(currentToken, "http://www.w3.org/1999/xhtml" /* HTML */);
      this.tokenizer.state = nextTokenizerState;
      this.originalInsertionMode = this.insertionMode;
      this.insertionMode = 7 /* TEXT */;
    }
    switchToPlaintextParsing() {
      this.insertionMode = 7 /* TEXT */;
      this.originalInsertionMode = 6 /* IN_BODY */;
      this.tokenizer.state = TokenizerMode.PLAINTEXT;
    }
    //Fragment parsing
    /** @protected */
    _getAdjustedCurrentElement() {
      return this.openElements.stackTop === 0 && this.fragmentContext ? this.fragmentContext : this.openElements.current;
    }
    /** @protected */
    _findFormInFragmentContext() {
      let node = this.fragmentContext;
      while (node) {
        if (this.treeAdapter.getTagName(node) === "form" /* FORM */) {
          this.formElement = node;
          break;
        }
        node = this.treeAdapter.getParentNode(node);
      }
    }
    _initTokenizerForFragmentParsing() {
      if (!this.fragmentContext || this.treeAdapter.getNamespaceURI(this.fragmentContext) !== "http://www.w3.org/1999/xhtml" /* HTML */) {
        return;
      }
      switch (this.fragmentContextID) {
        case 114 /* TITLE */:
        case 109 /* TEXTAREA */: {
          this.tokenizer.state = TokenizerMode.RCDATA;
          break;
        }
        case 102 /* STYLE */:
        case 123 /* XMP */:
        case 56 /* IFRAME */:
        case 77 /* NOEMBED */:
        case 76 /* NOFRAMES */:
        case 78 /* NOSCRIPT */: {
          this.tokenizer.state = TokenizerMode.RAWTEXT;
          break;
        }
        case 93 /* SCRIPT */: {
          this.tokenizer.state = TokenizerMode.SCRIPT_DATA;
          break;
        }
        case 85 /* PLAINTEXT */: {
          this.tokenizer.state = TokenizerMode.PLAINTEXT;
          break;
        }
        default:
      }
    }
    //Tree mutation
    /** @protected */
    _setDocumentType(token) {
      const name = token.name || "";
      const publicId = token.publicId || "";
      const systemId = token.systemId || "";
      this.treeAdapter.setDocumentType(this.document, name, publicId, systemId);
      if (token.location) {
        const documentChildren = this.treeAdapter.getChildNodes(this.document);
        const docTypeNode = documentChildren.find((node) => this.treeAdapter.isDocumentTypeNode(node));
        if (docTypeNode) {
          this.treeAdapter.setNodeSourceCodeLocation(docTypeNode, token.location);
        }
      }
    }
    /** @protected */
    _attachElementToTree(element, location, insertionLocation) {
      if (this.options.sourceCodeLocationInfo) {
        const loc = location && {
          ...location,
          startTag: location
        };
        this.treeAdapter.setNodeSourceCodeLocation(element, loc);
      }
      if (insertionLocation.beforeElement) {
        this.treeAdapter.insertBefore(insertionLocation.parent, element, insertionLocation.beforeElement);
      } else {
        this.treeAdapter.appendChild(insertionLocation.parent, element);
      }
    }
    /** @protected */
    _getInsertionLocation() {
      if (this._shouldFosterParentOnInsertion()) {
        return this._findFosterParentingLocation();
      }
      return {
        parent: this.openElements.currentTmplContentOrNode ?? this.document,
        beforeElement: null
      };
    }
    /** @protected */
    _createElement(tagName, namespaceURI, attrs, intendedParent) {
      const element = this.treeAdapter.createElement(tagName, namespaceURI, attrs);
      this.treeAdapter.onElementCreated?.(element, {
        intendedParent,
        formElement: this.formElement,
        isInTemplate: this.openElements.tmplCount > 0
      });
      return element;
    }
    /**
     * For self-closing tags. Add an element to the tree, but skip adding it
     * to the stack.
     */
    /** @protected */
    _appendElement(token, namespaceURI) {
      const insertionLocation = this._getInsertionLocation();
      const element = this._createElement(token.tagName, namespaceURI, token.attrs, insertionLocation.parent);
      this._attachElementToTree(element, token.location, insertionLocation);
    }
    /** @protected */
    _insertElement(token, namespaceURI) {
      const insertionLocation = this._getInsertionLocation();
      const element = this._createElement(token.tagName, namespaceURI, token.attrs, insertionLocation.parent);
      this._attachElementToTree(element, token.location, insertionLocation);
      this.openElements.push(element, token.tagID);
    }
    /** @protected */
    _insertFakeElement(tagName, tagID) {
      const insertionLocation = this._getInsertionLocation();
      const element = this._createElement(tagName, "http://www.w3.org/1999/xhtml" /* HTML */, [], insertionLocation.parent);
      this._attachElementToTree(element, null, insertionLocation);
      this.openElements.push(element, tagID);
    }
    /** @protected */
    _insertTemplate(token) {
      const insertionLocation = this._getInsertionLocation();
      const tmpl = this._createElement(token.tagName, "http://www.w3.org/1999/xhtml" /* HTML */, token.attrs, insertionLocation.parent);
      const content = this.treeAdapter.createDocumentFragment();
      this.treeAdapter.setTemplateContent(tmpl, content);
      this._attachElementToTree(tmpl, token.location, insertionLocation);
      this.openElements.push(tmpl, token.tagID);
      if (this.options.sourceCodeLocationInfo) this.treeAdapter.setNodeSourceCodeLocation(content, null);
    }
    /** @protected */
    _insertFakeRootElement() {
      const element = this._createElement("html" /* HTML */, "http://www.w3.org/1999/xhtml" /* HTML */, [], this.document);
      if (this.options.sourceCodeLocationInfo) this.treeAdapter.setNodeSourceCodeLocation(element, null);
      this.treeAdapter.appendChild(this.document, element);
      this.openElements.push(element, 51 /* HTML */);
    }
    /** @protected */
    _appendCommentNode(token, parent) {
      const commentNode = this.treeAdapter.createCommentNode(token.data);
      this.treeAdapter.appendChild(parent, commentNode);
      if (this.options.sourceCodeLocationInfo) {
        this.treeAdapter.setNodeSourceCodeLocation(commentNode, token.location);
      }
    }
    /** @protected */
    _insertCharacters(token) {
      let parent;
      let beforeElement;
      if (this._shouldFosterParentOnInsertion()) {
        ({ parent, beforeElement } = this._findFosterParentingLocation());
        if (beforeElement) {
          this.treeAdapter.insertTextBefore(parent, token.chars, beforeElement);
        } else {
          this.treeAdapter.insertText(parent, token.chars);
        }
      } else {
        parent = this.openElements.currentTmplContentOrNode;
        this.treeAdapter.insertText(parent, token.chars);
      }
      if (!token.location) return;
      const siblings = this.treeAdapter.getChildNodes(parent);
      const textNodeIdx = beforeElement ? siblings.lastIndexOf(beforeElement) : siblings.length;
      const textNode = siblings[textNodeIdx - 1];
      const tnLoc = this.treeAdapter.getNodeSourceCodeLocation(textNode);
      if (tnLoc) {
        const { endLine, endCol, endOffset } = token.location;
        this.treeAdapter.updateNodeSourceCodeLocation(textNode, { endLine, endCol, endOffset });
      } else if (this.options.sourceCodeLocationInfo) {
        this.treeAdapter.setNodeSourceCodeLocation(textNode, token.location);
      }
    }
    /** @protected */
    _adoptNodes(donor, recipient) {
      for (let child = this.treeAdapter.getFirstChild(donor); child; child = this.treeAdapter.getFirstChild(donor)) {
        this.treeAdapter.detachNode(child);
        this.treeAdapter.appendChild(recipient, child);
      }
    }
    /** @protected */
    _setEndLocation(element, closingToken) {
      if (this.treeAdapter.getNodeSourceCodeLocation(element) && closingToken.location) {
        const ctLoc = closingToken.location;
        const tn = this.treeAdapter.getTagName(element);
        const endLoc = (
          // NOTE: For cases like <p> <p> </p> - First 'p' closes without a closing
          // tag and for cases like <td> <p> </td> - 'p' closes without a closing tag.
          closingToken.type === 4 /* END_TAG */ && tn === closingToken.tagName ? {
            endTag: { ...ctLoc },
            endLine: ctLoc.endLine,
            endCol: ctLoc.endCol,
            endOffset: ctLoc.endOffset
          } : {
            endLine: ctLoc.startLine,
            endCol: ctLoc.startCol,
            endOffset: ctLoc.startOffset
          }
        );
        this.treeAdapter.updateNodeSourceCodeLocation(element, endLoc);
      }
    }
    //Token processing
    shouldProcessStartTagTokenInForeignContent(token) {
      if (!this.currentNotInHTML) return false;
      let current;
      let currentTagId;
      if (this.openElements.stackTop === 0 && this.fragmentContext) {
        current = this.fragmentContext;
        currentTagId = this.fragmentContextID;
      } else {
        ({ current, currentTagId } = this.openElements);
      }
      if (token.tagID === 120 /* SVG */ && this.treeAdapter.getTagName(current) === "annotation-xml" /* ANNOTATION_XML */ && this.treeAdapter.getNamespaceURI(current) === "http://www.w3.org/1998/Math/MathML" /* MATHML */) {
        return false;
      }
      return (
        // Check that `current` is not an integration point for HTML or MathML elements.
        this.tokenizer.inForeignNode || // If it _is_ an integration point, then we might have to check that it is not an HTML
        // integration point.
        (token.tagID === 68 /* MGLYPH */ || token.tagID === 63 /* MALIGNMARK */) && currentTagId !== void 0 && !this._isIntegrationPoint(currentTagId, current, "http://www.w3.org/1999/xhtml" /* HTML */)
      );
    }
    /** @protected */
    _processToken(token) {
      switch (token.type) {
        case 0 /* CHARACTER */: {
          this.onCharacter(token);
          break;
        }
        case 1 /* NULL_CHARACTER */: {
          this.onNullCharacter(token);
          break;
        }
        case 5 /* COMMENT */: {
          this.onComment(token);
          break;
        }
        case 6 /* DOCTYPE */: {
          this.onDoctype(token);
          break;
        }
        case 3 /* START_TAG */: {
          this._processStartTag(token);
          break;
        }
        case 4 /* END_TAG */: {
          this.onEndTag(token);
          break;
        }
        case 7 /* EOF */: {
          this.onEof(token);
          break;
        }
        case 2 /* WHITESPACE_CHARACTER */: {
          this.onWhitespaceCharacter(token);
          break;
        }
      }
    }
    //Integration points
    /** @protected */
    _isIntegrationPoint(tid, element, foreignNS) {
      const ns = this.treeAdapter.getNamespaceURI(element);
      const attrs = this.treeAdapter.getAttrList(element);
      return isIntegrationPoint(tid, ns, attrs, foreignNS);
    }
    //Active formatting elements reconstruction
    /** @protected */
    _reconstructActiveFormattingElements() {
      const listLength = this.activeFormattingElements.entries.length;
      if (listLength) {
        const endIndex = this.activeFormattingElements.entries.findIndex(
          (entry) => entry.type === 0 /* Marker */ || this.openElements.contains(entry.element)
        );
        const unopenIdx = endIndex === -1 ? listLength - 1 : endIndex - 1;
        for (let i = unopenIdx; i >= 0; i--) {
          const entry = this.activeFormattingElements.entries[i];
          this._insertElement(entry.token, this.treeAdapter.getNamespaceURI(entry.element));
          entry.element = this.openElements.current;
        }
      }
    }
    //Close elements
    /** @protected */
    _closeTableCell() {
      this.openElements.generateImpliedEndTags();
      this.openElements.popUntilTableCellPopped();
      this.activeFormattingElements.clearToLastMarker();
      this.insertionMode = 13 /* IN_ROW */;
    }
    /** @protected */
    _closePElement() {
      this.openElements.generateImpliedEndTagsWithExclusion(83 /* P */);
      this.openElements.popUntilTagNamePopped(83 /* P */);
    }
    //Insertion modes
    /** @protected */
    _resetInsertionMode() {
      for (let i = this.openElements.stackTop; i >= 0; i--) {
        switch (i === 0 && this.fragmentContext ? this.fragmentContextID : this.openElements.tagIDs[i]) {
          case 115 /* TR */: {
            this.insertionMode = 13 /* IN_ROW */;
            return;
          }
          case 107 /* TBODY */:
          case 113 /* THEAD */:
          case 110 /* TFOOT */: {
            this.insertionMode = 12 /* IN_TABLE_BODY */;
            return;
          }
          case 17 /* CAPTION */: {
            this.insertionMode = 10 /* IN_CAPTION */;
            return;
          }
          case 21 /* COLGROUP */: {
            this.insertionMode = 11 /* IN_COLUMN_GROUP */;
            return;
          }
          case 106 /* TABLE */: {
            this.insertionMode = 8 /* IN_TABLE */;
            return;
          }
          case 14 /* BODY */: {
            this.insertionMode = 6 /* IN_BODY */;
            return;
          }
          case 40 /* FRAMESET */: {
            this.insertionMode = 19 /* IN_FRAMESET */;
            return;
          }
          case 96 /* SELECT */: {
            this._resetInsertionModeForSelect(i);
            return;
          }
          case 108 /* TEMPLATE */: {
            this.insertionMode = this.tmplInsertionModeStack[0];
            return;
          }
          case 51 /* HTML */: {
            this.insertionMode = this.headElement ? 5 /* AFTER_HEAD */ : 2 /* BEFORE_HEAD */;
            return;
          }
          case 111 /* TD */:
          case 112 /* TH */: {
            if (i > 0) {
              this.insertionMode = 14 /* IN_CELL */;
              return;
            }
            break;
          }
          case 47 /* HEAD */: {
            if (i > 0) {
              this.insertionMode = 3 /* IN_HEAD */;
              return;
            }
            break;
          }
        }
      }
      this.insertionMode = 6 /* IN_BODY */;
    }
    /** @protected */
    _resetInsertionModeForSelect(selectIdx) {
      if (selectIdx > 0) {
        for (let i = selectIdx - 1; i > 0; i--) {
          const tn = this.openElements.tagIDs[i];
          if (tn === 108 /* TEMPLATE */) {
            break;
          } else if (tn === 106 /* TABLE */) {
            this.insertionMode = 16 /* IN_SELECT_IN_TABLE */;
            return;
          }
        }
      }
      this.insertionMode = 15 /* IN_SELECT */;
    }
    //Foster parenting
    /** @protected */
    _isElementCausesFosterParenting(tn) {
      return TABLE_STRUCTURE_TAGS.has(tn);
    }
    /** @protected */
    _shouldFosterParentOnInsertion() {
      return this.fosterParentingEnabled && this.openElements.currentTagId !== void 0 && this._isElementCausesFosterParenting(this.openElements.currentTagId);
    }
    /** @protected */
    _findFosterParentingLocation() {
      for (let i = this.openElements.stackTop; i >= 0; i--) {
        const openElement = this.openElements.items[i];
        switch (this.openElements.tagIDs[i]) {
          case 108 /* TEMPLATE */: {
            if (this.treeAdapter.getNamespaceURI(openElement) === "http://www.w3.org/1999/xhtml" /* HTML */) {
              return { parent: this.treeAdapter.getTemplateContent(openElement), beforeElement: null };
            }
            break;
          }
          case 106 /* TABLE */: {
            const parent = this.treeAdapter.getParentNode(openElement);
            if (parent) {
              return { parent, beforeElement: openElement };
            }
            return { parent: this.openElements.items[i - 1], beforeElement: null };
          }
          default:
        }
      }
      return { parent: this.openElements.items[0], beforeElement: null };
    }
    /** @protected */
    _fosterParentElement(element) {
      const location = this._findFosterParentingLocation();
      if (location.beforeElement) {
        this.treeAdapter.insertBefore(location.parent, element, location.beforeElement);
      } else {
        this.treeAdapter.appendChild(location.parent, element);
      }
    }
    //Special elements
    /** @protected */
    _isSpecialElement(element, id) {
      const ns = this.treeAdapter.getNamespaceURI(element);
      return SPECIAL_ELEMENTS[ns].has(id);
    }
    /** @internal */
    onCharacter(token) {
      this.skipNextNewLine = false;
      if (this.tokenizer.inForeignNode) {
        characterInForeignContent(this, token);
        return;
      }
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          tokenInInitialMode(this, token);
          break;
        }
        case 1 /* BEFORE_HTML */: {
          tokenBeforeHtml(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */: {
          tokenBeforeHead(this, token);
          break;
        }
        case 3 /* IN_HEAD */: {
          tokenInHead(this, token);
          break;
        }
        case 4 /* IN_HEAD_NO_SCRIPT */: {
          tokenInHeadNoScript(this, token);
          break;
        }
        case 5 /* AFTER_HEAD */: {
          tokenAfterHead(this, token);
          break;
        }
        case 6 /* IN_BODY */:
        case 10 /* IN_CAPTION */:
        case 14 /* IN_CELL */:
        case 17 /* IN_TEMPLATE */: {
          characterInBody(this, token);
          break;
        }
        case 7 /* TEXT */:
        case 15 /* IN_SELECT */:
        case 16 /* IN_SELECT_IN_TABLE */: {
          this._insertCharacters(token);
          break;
        }
        case 8 /* IN_TABLE */:
        case 12 /* IN_TABLE_BODY */:
        case 13 /* IN_ROW */: {
          characterInTable(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          characterInTableText(this, token);
          break;
        }
        case 11 /* IN_COLUMN_GROUP */: {
          tokenInColumnGroup(this, token);
          break;
        }
        case 18 /* AFTER_BODY */: {
          tokenAfterBody(this, token);
          break;
        }
        case 21 /* AFTER_AFTER_BODY */: {
          tokenAfterAfterBody(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onNullCharacter(token) {
      this.skipNextNewLine = false;
      if (this.tokenizer.inForeignNode) {
        nullCharacterInForeignContent(this, token);
        return;
      }
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          tokenInInitialMode(this, token);
          break;
        }
        case 1 /* BEFORE_HTML */: {
          tokenBeforeHtml(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */: {
          tokenBeforeHead(this, token);
          break;
        }
        case 3 /* IN_HEAD */: {
          tokenInHead(this, token);
          break;
        }
        case 4 /* IN_HEAD_NO_SCRIPT */: {
          tokenInHeadNoScript(this, token);
          break;
        }
        case 5 /* AFTER_HEAD */: {
          tokenAfterHead(this, token);
          break;
        }
        case 7 /* TEXT */: {
          this._insertCharacters(token);
          break;
        }
        case 8 /* IN_TABLE */:
        case 12 /* IN_TABLE_BODY */:
        case 13 /* IN_ROW */: {
          characterInTable(this, token);
          break;
        }
        case 11 /* IN_COLUMN_GROUP */: {
          tokenInColumnGroup(this, token);
          break;
        }
        case 18 /* AFTER_BODY */: {
          tokenAfterBody(this, token);
          break;
        }
        case 21 /* AFTER_AFTER_BODY */: {
          tokenAfterAfterBody(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onComment(token) {
      this.skipNextNewLine = false;
      if (this.currentNotInHTML) {
        appendComment(this, token);
        return;
      }
      switch (this.insertionMode) {
        case 0 /* INITIAL */:
        case 1 /* BEFORE_HTML */:
        case 2 /* BEFORE_HEAD */:
        case 3 /* IN_HEAD */:
        case 4 /* IN_HEAD_NO_SCRIPT */:
        case 5 /* AFTER_HEAD */:
        case 6 /* IN_BODY */:
        case 8 /* IN_TABLE */:
        case 10 /* IN_CAPTION */:
        case 11 /* IN_COLUMN_GROUP */:
        case 12 /* IN_TABLE_BODY */:
        case 13 /* IN_ROW */:
        case 14 /* IN_CELL */:
        case 15 /* IN_SELECT */:
        case 16 /* IN_SELECT_IN_TABLE */:
        case 17 /* IN_TEMPLATE */:
        case 19 /* IN_FRAMESET */:
        case 20 /* AFTER_FRAMESET */: {
          appendComment(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          tokenInTableText(this, token);
          break;
        }
        case 18 /* AFTER_BODY */: {
          appendCommentToRootHtmlElement(this, token);
          break;
        }
        case 21 /* AFTER_AFTER_BODY */:
        case 22 /* AFTER_AFTER_FRAMESET */: {
          appendCommentToDocument(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onDoctype(token) {
      this.skipNextNewLine = false;
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          doctypeInInitialMode(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */:
        case 3 /* IN_HEAD */:
        case 4 /* IN_HEAD_NO_SCRIPT */:
        case 5 /* AFTER_HEAD */: {
          this._err(token, "misplaced-doctype" /* misplacedDoctype */);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          tokenInTableText(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onStartTag(token) {
      this.skipNextNewLine = false;
      this.currentToken = token;
      this._processStartTag(token);
      if (token.selfClosing && !token.ackSelfClosing) {
        this._err(token, "non-void-html-element-start-tag-with-trailing-solidus" /* nonVoidHtmlElementStartTagWithTrailingSolidus */);
      }
    }
    /**
     * Processes a given start tag.
     *
     * `onStartTag` checks if a self-closing tag was recognized. When a token
     * is moved inbetween multiple insertion modes, this check for self-closing
     * could lead to false positives. To avoid this, `_processStartTag` is used
     * for nested calls.
     *
     * @param token The token to process.
     * @protected
     */
    _processStartTag(token) {
      if (this.shouldProcessStartTagTokenInForeignContent(token)) {
        startTagInForeignContent(this, token);
      } else {
        this._startTagOutsideForeignContent(token);
      }
    }
    /** @protected */
    _startTagOutsideForeignContent(token) {
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          tokenInInitialMode(this, token);
          break;
        }
        case 1 /* BEFORE_HTML */: {
          startTagBeforeHtml(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */: {
          startTagBeforeHead(this, token);
          break;
        }
        case 3 /* IN_HEAD */: {
          startTagInHead(this, token);
          break;
        }
        case 4 /* IN_HEAD_NO_SCRIPT */: {
          startTagInHeadNoScript(this, token);
          break;
        }
        case 5 /* AFTER_HEAD */: {
          startTagAfterHead(this, token);
          break;
        }
        case 6 /* IN_BODY */: {
          startTagInBody(this, token);
          break;
        }
        case 8 /* IN_TABLE */: {
          startTagInTable(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          tokenInTableText(this, token);
          break;
        }
        case 10 /* IN_CAPTION */: {
          startTagInCaption(this, token);
          break;
        }
        case 11 /* IN_COLUMN_GROUP */: {
          startTagInColumnGroup(this, token);
          break;
        }
        case 12 /* IN_TABLE_BODY */: {
          startTagInTableBody(this, token);
          break;
        }
        case 13 /* IN_ROW */: {
          startTagInRow(this, token);
          break;
        }
        case 14 /* IN_CELL */: {
          startTagInCell(this, token);
          break;
        }
        case 15 /* IN_SELECT */: {
          startTagInSelect(this, token);
          break;
        }
        case 16 /* IN_SELECT_IN_TABLE */: {
          startTagInSelectInTable(this, token);
          break;
        }
        case 17 /* IN_TEMPLATE */: {
          startTagInTemplate(this, token);
          break;
        }
        case 18 /* AFTER_BODY */: {
          startTagAfterBody(this, token);
          break;
        }
        case 19 /* IN_FRAMESET */: {
          startTagInFrameset(this, token);
          break;
        }
        case 20 /* AFTER_FRAMESET */: {
          startTagAfterFrameset(this, token);
          break;
        }
        case 21 /* AFTER_AFTER_BODY */: {
          startTagAfterAfterBody(this, token);
          break;
        }
        case 22 /* AFTER_AFTER_FRAMESET */: {
          startTagAfterAfterFrameset(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onEndTag(token) {
      this.skipNextNewLine = false;
      this.currentToken = token;
      if (this.currentNotInHTML) {
        endTagInForeignContent(this, token);
      } else {
        this._endTagOutsideForeignContent(token);
      }
    }
    /** @protected */
    _endTagOutsideForeignContent(token) {
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          tokenInInitialMode(this, token);
          break;
        }
        case 1 /* BEFORE_HTML */: {
          endTagBeforeHtml(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */: {
          endTagBeforeHead(this, token);
          break;
        }
        case 3 /* IN_HEAD */: {
          endTagInHead(this, token);
          break;
        }
        case 4 /* IN_HEAD_NO_SCRIPT */: {
          endTagInHeadNoScript(this, token);
          break;
        }
        case 5 /* AFTER_HEAD */: {
          endTagAfterHead(this, token);
          break;
        }
        case 6 /* IN_BODY */: {
          endTagInBody(this, token);
          break;
        }
        case 7 /* TEXT */: {
          endTagInText(this, token);
          break;
        }
        case 8 /* IN_TABLE */: {
          endTagInTable(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          tokenInTableText(this, token);
          break;
        }
        case 10 /* IN_CAPTION */: {
          endTagInCaption(this, token);
          break;
        }
        case 11 /* IN_COLUMN_GROUP */: {
          endTagInColumnGroup(this, token);
          break;
        }
        case 12 /* IN_TABLE_BODY */: {
          endTagInTableBody(this, token);
          break;
        }
        case 13 /* IN_ROW */: {
          endTagInRow(this, token);
          break;
        }
        case 14 /* IN_CELL */: {
          endTagInCell(this, token);
          break;
        }
        case 15 /* IN_SELECT */: {
          endTagInSelect(this, token);
          break;
        }
        case 16 /* IN_SELECT_IN_TABLE */: {
          endTagInSelectInTable(this, token);
          break;
        }
        case 17 /* IN_TEMPLATE */: {
          endTagInTemplate(this, token);
          break;
        }
        case 18 /* AFTER_BODY */: {
          endTagAfterBody(this, token);
          break;
        }
        case 19 /* IN_FRAMESET */: {
          endTagInFrameset(this, token);
          break;
        }
        case 20 /* AFTER_FRAMESET */: {
          endTagAfterFrameset(this, token);
          break;
        }
        case 21 /* AFTER_AFTER_BODY */: {
          tokenAfterAfterBody(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onEof(token) {
      switch (this.insertionMode) {
        case 0 /* INITIAL */: {
          tokenInInitialMode(this, token);
          break;
        }
        case 1 /* BEFORE_HTML */: {
          tokenBeforeHtml(this, token);
          break;
        }
        case 2 /* BEFORE_HEAD */: {
          tokenBeforeHead(this, token);
          break;
        }
        case 3 /* IN_HEAD */: {
          tokenInHead(this, token);
          break;
        }
        case 4 /* IN_HEAD_NO_SCRIPT */: {
          tokenInHeadNoScript(this, token);
          break;
        }
        case 5 /* AFTER_HEAD */: {
          tokenAfterHead(this, token);
          break;
        }
        case 6 /* IN_BODY */:
        case 8 /* IN_TABLE */:
        case 10 /* IN_CAPTION */:
        case 11 /* IN_COLUMN_GROUP */:
        case 12 /* IN_TABLE_BODY */:
        case 13 /* IN_ROW */:
        case 14 /* IN_CELL */:
        case 15 /* IN_SELECT */:
        case 16 /* IN_SELECT_IN_TABLE */: {
          eofInBody(this, token);
          break;
        }
        case 7 /* TEXT */: {
          eofInText(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          tokenInTableText(this, token);
          break;
        }
        case 17 /* IN_TEMPLATE */: {
          eofInTemplate(this, token);
          break;
        }
        case 18 /* AFTER_BODY */:
        case 19 /* IN_FRAMESET */:
        case 20 /* AFTER_FRAMESET */:
        case 21 /* AFTER_AFTER_BODY */:
        case 22 /* AFTER_AFTER_FRAMESET */: {
          stopParsing(this, token);
          break;
        }
        default:
      }
    }
    /** @internal */
    onWhitespaceCharacter(token) {
      if (this.skipNextNewLine) {
        this.skipNextNewLine = false;
        if (token.chars.charCodeAt(0) === 10 /* LINE_FEED */) {
          if (token.chars.length === 1) {
            return;
          }
          token.chars = token.chars.substr(1);
        }
      }
      if (this.tokenizer.inForeignNode) {
        this._insertCharacters(token);
        return;
      }
      switch (this.insertionMode) {
        case 3 /* IN_HEAD */:
        case 4 /* IN_HEAD_NO_SCRIPT */:
        case 5 /* AFTER_HEAD */:
        case 7 /* TEXT */:
        case 11 /* IN_COLUMN_GROUP */:
        case 15 /* IN_SELECT */:
        case 16 /* IN_SELECT_IN_TABLE */:
        case 19 /* IN_FRAMESET */:
        case 20 /* AFTER_FRAMESET */: {
          this._insertCharacters(token);
          break;
        }
        case 6 /* IN_BODY */:
        case 10 /* IN_CAPTION */:
        case 14 /* IN_CELL */:
        case 17 /* IN_TEMPLATE */:
        case 18 /* AFTER_BODY */:
        case 21 /* AFTER_AFTER_BODY */:
        case 22 /* AFTER_AFTER_FRAMESET */: {
          whitespaceCharacterInBody(this, token);
          break;
        }
        case 8 /* IN_TABLE */:
        case 12 /* IN_TABLE_BODY */:
        case 13 /* IN_ROW */: {
          characterInTable(this, token);
          break;
        }
        case 9 /* IN_TABLE_TEXT */: {
          whitespaceCharacterInTableText(this, token);
          break;
        }
        default:
      }
    }
  };
  function aaObtainFormattingElementEntry(p, token) {
    let formattingElementEntry = p.activeFormattingElements.getElementEntryInScopeWithTagName(token.tagName);
    if (formattingElementEntry) {
      if (!p.openElements.contains(formattingElementEntry.element)) {
        p.activeFormattingElements.removeEntry(formattingElementEntry);
        formattingElementEntry = null;
      } else if (!p.openElements.hasInScope(token.tagID)) {
        formattingElementEntry = null;
      }
    } else {
      genericEndTagInBody(p, token);
    }
    return formattingElementEntry;
  }
  function aaObtainFurthestBlock(p, formattingElementEntry) {
    let furthestBlock = null;
    let idx = p.openElements.stackTop;
    for (; idx >= 0; idx--) {
      const element = p.openElements.items[idx];
      if (element === formattingElementEntry.element) {
        break;
      }
      if (p._isSpecialElement(element, p.openElements.tagIDs[idx])) {
        furthestBlock = element;
      }
    }
    if (!furthestBlock) {
      p.openElements.shortenToLength(Math.max(idx, 0));
      p.activeFormattingElements.removeEntry(formattingElementEntry);
    }
    return furthestBlock;
  }
  function aaInnerLoop(p, furthestBlock, formattingElement) {
    let lastElement = furthestBlock;
    let nextElement = p.openElements.getCommonAncestor(furthestBlock);
    const intendedParent = p.openElements.getCommonAncestor(formattingElement);
    for (let i = 0, element = nextElement; element !== formattingElement; i++, element = nextElement) {
      nextElement = p.openElements.getCommonAncestor(element);
      const elementEntry = p.activeFormattingElements.getElementEntry(element);
      const counterOverflow = elementEntry && i >= AA_INNER_LOOP_ITER;
      const shouldRemoveFromOpenElements = !elementEntry || counterOverflow;
      if (shouldRemoveFromOpenElements) {
        if (counterOverflow) {
          p.activeFormattingElements.removeEntry(elementEntry);
        }
        p.openElements.remove(element);
      } else {
        element = aaRecreateElementFromEntry(p, elementEntry, intendedParent);
        if (lastElement === furthestBlock) {
          p.activeFormattingElements.bookmark = elementEntry;
        }
        p.treeAdapter.detachNode(lastElement);
        p.treeAdapter.appendChild(element, lastElement);
        lastElement = element;
      }
    }
    return lastElement;
  }
  function aaRecreateElementFromEntry(p, elementEntry, intendedParent) {
    const ns = p.treeAdapter.getNamespaceURI(elementEntry.element);
    const newElement = p._createElement(elementEntry.token.tagName, ns, elementEntry.token.attrs, intendedParent);
    p.openElements.replace(elementEntry.element, newElement);
    elementEntry.element = newElement;
    return newElement;
  }
  function aaInsertLastNodeInCommonAncestor(p, commonAncestor, lastElement) {
    const tn = p.treeAdapter.getTagName(commonAncestor);
    const tid = getTagID(tn);
    if (p._isElementCausesFosterParenting(tid)) {
      p._fosterParentElement(lastElement);
    } else {
      const ns = p.treeAdapter.getNamespaceURI(commonAncestor);
      if (tid === 108 /* TEMPLATE */ && ns === "http://www.w3.org/1999/xhtml" /* HTML */) {
        commonAncestor = p.treeAdapter.getTemplateContent(commonAncestor);
      }
      p.treeAdapter.appendChild(commonAncestor, lastElement);
    }
  }
  function aaReplaceFormattingElement(p, furthestBlock, formattingElementEntry) {
    const ns = p.treeAdapter.getNamespaceURI(formattingElementEntry.element);
    const { token } = formattingElementEntry;
    const newElement = p._createElement(token.tagName, ns, token.attrs, furthestBlock);
    p._adoptNodes(furthestBlock, newElement);
    p.treeAdapter.appendChild(furthestBlock, newElement);
    p.activeFormattingElements.insertElementAfterBookmark(newElement, token);
    p.activeFormattingElements.removeEntry(formattingElementEntry);
    p.openElements.remove(formattingElementEntry.element);
    p.openElements.insertAfter(furthestBlock, newElement, token.tagID);
  }
  function callAdoptionAgency(p, token) {
    for (let i = 0; i < AA_OUTER_LOOP_ITER; i++) {
      const formattingElementEntry = aaObtainFormattingElementEntry(p, token);
      if (!formattingElementEntry) {
        break;
      }
      const furthestBlock = aaObtainFurthestBlock(p, formattingElementEntry);
      if (!furthestBlock) {
        break;
      }
      p.activeFormattingElements.bookmark = formattingElementEntry;
      const lastElement = aaInnerLoop(p, furthestBlock, formattingElementEntry.element);
      const commonAncestor = p.openElements.getCommonAncestor(formattingElementEntry.element);
      p.treeAdapter.detachNode(lastElement);
      if (commonAncestor) aaInsertLastNodeInCommonAncestor(p, commonAncestor, lastElement);
      aaReplaceFormattingElement(p, furthestBlock, formattingElementEntry);
    }
  }
  function appendComment(p, token) {
    p._appendCommentNode(token, p.openElements.currentTmplContentOrNode);
  }
  function appendCommentToRootHtmlElement(p, token) {
    p._appendCommentNode(token, p.openElements.items[0]);
  }
  function appendCommentToDocument(p, token) {
    p._appendCommentNode(token, p.document);
  }
  function stopParsing(p, token) {
    p.stopped = true;
    if (token.location) {
      const target = p.fragmentContext ? 0 : 2;
      for (let i = p.openElements.stackTop; i >= target; i--) {
        p._setEndLocation(p.openElements.items[i], token);
      }
      if (!p.fragmentContext && p.openElements.stackTop >= 0) {
        const htmlElement = p.openElements.items[0];
        const htmlLocation = p.treeAdapter.getNodeSourceCodeLocation(htmlElement);
        if (htmlLocation && !htmlLocation.endTag) {
          p._setEndLocation(htmlElement, token);
          if (p.openElements.stackTop >= 1) {
            const bodyElement = p.openElements.items[1];
            const bodyLocation = p.treeAdapter.getNodeSourceCodeLocation(bodyElement);
            if (bodyLocation && !bodyLocation.endTag) {
              p._setEndLocation(bodyElement, token);
            }
          }
        }
      }
    }
  }
  function doctypeInInitialMode(p, token) {
    p._setDocumentType(token);
    const mode = token.forceQuirks ? "quirks" /* QUIRKS */ : getDocumentMode(token);
    if (!isConforming(token)) {
      p._err(token, "non-conforming-doctype" /* nonConformingDoctype */);
    }
    p.treeAdapter.setDocumentMode(p.document, mode);
    p.insertionMode = 1 /* BEFORE_HTML */;
  }
  function tokenInInitialMode(p, token) {
    p._err(token, "missing-doctype" /* missingDoctype */, true);
    p.treeAdapter.setDocumentMode(p.document, "quirks" /* QUIRKS */);
    p.insertionMode = 1 /* BEFORE_HTML */;
    p._processToken(token);
  }
  function startTagBeforeHtml(p, token) {
    if (token.tagID === 51 /* HTML */) {
      p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
      p.insertionMode = 2 /* BEFORE_HEAD */;
    } else {
      tokenBeforeHtml(p, token);
    }
  }
  function endTagBeforeHtml(p, token) {
    const tn = token.tagID;
    if (tn === 51 /* HTML */ || tn === 47 /* HEAD */ || tn === 14 /* BODY */ || tn === 15 /* BR */) {
      tokenBeforeHtml(p, token);
    }
  }
  function tokenBeforeHtml(p, token) {
    p._insertFakeRootElement();
    p.insertionMode = 2 /* BEFORE_HEAD */;
    p._processToken(token);
  }
  function startTagBeforeHead(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 47 /* HEAD */: {
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        p.headElement = p.openElements.current;
        p.insertionMode = 3 /* IN_HEAD */;
        break;
      }
      default: {
        tokenBeforeHead(p, token);
      }
    }
  }
  function endTagBeforeHead(p, token) {
    const tn = token.tagID;
    if (tn === 47 /* HEAD */ || tn === 14 /* BODY */ || tn === 51 /* HTML */ || tn === 15 /* BR */) {
      tokenBeforeHead(p, token);
    } else {
      p._err(token, "end-tag-without-matching-open-element" /* endTagWithoutMatchingOpenElement */);
    }
  }
  function tokenBeforeHead(p, token) {
    p._insertFakeElement("head" /* HEAD */, 47 /* HEAD */);
    p.headElement = p.openElements.current;
    p.insertionMode = 3 /* IN_HEAD */;
    p._processToken(token);
  }
  function startTagInHead(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 9 /* BASE */:
      case 10 /* BASEFONT */:
      case 11 /* BGSOUND */:
      case 60 /* LINK */:
      case 67 /* META */: {
        p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        token.ackSelfClosing = true;
        break;
      }
      case 114 /* TITLE */: {
        p._switchToTextParsing(token, TokenizerMode.RCDATA);
        break;
      }
      case 78 /* NOSCRIPT */: {
        if (p.options.scriptingEnabled) {
          p._switchToTextParsing(token, TokenizerMode.RAWTEXT);
        } else {
          p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
          p.insertionMode = 4 /* IN_HEAD_NO_SCRIPT */;
        }
        break;
      }
      case 76 /* NOFRAMES */:
      case 102 /* STYLE */: {
        p._switchToTextParsing(token, TokenizerMode.RAWTEXT);
        break;
      }
      case 93 /* SCRIPT */: {
        p._switchToTextParsing(token, TokenizerMode.SCRIPT_DATA);
        break;
      }
      case 108 /* TEMPLATE */: {
        p._insertTemplate(token);
        p.activeFormattingElements.insertMarker();
        p.framesetOk = false;
        p.insertionMode = 17 /* IN_TEMPLATE */;
        p.tmplInsertionModeStack.unshift(17 /* IN_TEMPLATE */);
        break;
      }
      case 47 /* HEAD */: {
        p._err(token, "misplaced-start-tag-for-head-element" /* misplacedStartTagForHeadElement */);
        break;
      }
      default: {
        tokenInHead(p, token);
      }
    }
  }
  function endTagInHead(p, token) {
    switch (token.tagID) {
      case 47 /* HEAD */: {
        p.openElements.pop();
        p.insertionMode = 5 /* AFTER_HEAD */;
        break;
      }
      case 14 /* BODY */:
      case 15 /* BR */:
      case 51 /* HTML */: {
        tokenInHead(p, token);
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      default: {
        p._err(token, "end-tag-without-matching-open-element" /* endTagWithoutMatchingOpenElement */);
      }
    }
  }
  function templateEndTagInHead(p, token) {
    if (p.openElements.tmplCount > 0) {
      p.openElements.generateImpliedEndTagsThoroughly();
      if (p.openElements.currentTagId !== 108 /* TEMPLATE */) {
        p._err(token, "closing-of-element-with-open-child-elements" /* closingOfElementWithOpenChildElements */);
      }
      p.openElements.popUntilTagNamePopped(108 /* TEMPLATE */);
      p.activeFormattingElements.clearToLastMarker();
      p.tmplInsertionModeStack.shift();
      p._resetInsertionMode();
    } else {
      p._err(token, "end-tag-without-matching-open-element" /* endTagWithoutMatchingOpenElement */);
    }
  }
  function tokenInHead(p, token) {
    p.openElements.pop();
    p.insertionMode = 5 /* AFTER_HEAD */;
    p._processToken(token);
  }
  function startTagInHeadNoScript(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 10 /* BASEFONT */:
      case 11 /* BGSOUND */:
      case 47 /* HEAD */:
      case 60 /* LINK */:
      case 67 /* META */:
      case 76 /* NOFRAMES */:
      case 102 /* STYLE */: {
        startTagInHead(p, token);
        break;
      }
      case 78 /* NOSCRIPT */: {
        p._err(token, "nested-noscript-in-head" /* nestedNoscriptInHead */);
        break;
      }
      default: {
        tokenInHeadNoScript(p, token);
      }
    }
  }
  function endTagInHeadNoScript(p, token) {
    switch (token.tagID) {
      case 78 /* NOSCRIPT */: {
        p.openElements.pop();
        p.insertionMode = 3 /* IN_HEAD */;
        break;
      }
      case 15 /* BR */: {
        tokenInHeadNoScript(p, token);
        break;
      }
      default: {
        p._err(token, "end-tag-without-matching-open-element" /* endTagWithoutMatchingOpenElement */);
      }
    }
  }
  function tokenInHeadNoScript(p, token) {
    const errCode = token.type === 7 /* EOF */ ? "open-elements-left-after-eof" /* openElementsLeftAfterEof */ : "disallowed-content-in-noscript-in-head" /* disallowedContentInNoscriptInHead */;
    p._err(token, errCode);
    p.openElements.pop();
    p.insertionMode = 3 /* IN_HEAD */;
    p._processToken(token);
  }
  function startTagAfterHead(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 14 /* BODY */: {
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        p.framesetOk = false;
        p.insertionMode = 6 /* IN_BODY */;
        break;
      }
      case 40 /* FRAMESET */: {
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        p.insertionMode = 19 /* IN_FRAMESET */;
        break;
      }
      case 9 /* BASE */:
      case 10 /* BASEFONT */:
      case 11 /* BGSOUND */:
      case 60 /* LINK */:
      case 67 /* META */:
      case 76 /* NOFRAMES */:
      case 93 /* SCRIPT */:
      case 102 /* STYLE */:
      case 108 /* TEMPLATE */:
      case 114 /* TITLE */: {
        p._err(token, "abandoned-head-element-child" /* abandonedHeadElementChild */);
        p.openElements.push(p.headElement, 47 /* HEAD */);
        startTagInHead(p, token);
        p.openElements.remove(p.headElement);
        break;
      }
      case 47 /* HEAD */: {
        p._err(token, "misplaced-start-tag-for-head-element" /* misplacedStartTagForHeadElement */);
        break;
      }
      default: {
        tokenAfterHead(p, token);
      }
    }
  }
  function endTagAfterHead(p, token) {
    switch (token.tagID) {
      case 14 /* BODY */:
      case 51 /* HTML */:
      case 15 /* BR */: {
        tokenAfterHead(p, token);
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      default: {
        p._err(token, "end-tag-without-matching-open-element" /* endTagWithoutMatchingOpenElement */);
      }
    }
  }
  function tokenAfterHead(p, token) {
    p._insertFakeElement("body" /* BODY */, 14 /* BODY */);
    p.insertionMode = 6 /* IN_BODY */;
    modeInBody(p, token);
  }
  function modeInBody(p, token) {
    switch (token.type) {
      case 0 /* CHARACTER */: {
        characterInBody(p, token);
        break;
      }
      case 2 /* WHITESPACE_CHARACTER */: {
        whitespaceCharacterInBody(p, token);
        break;
      }
      case 5 /* COMMENT */: {
        appendComment(p, token);
        break;
      }
      case 3 /* START_TAG */: {
        startTagInBody(p, token);
        break;
      }
      case 4 /* END_TAG */: {
        endTagInBody(p, token);
        break;
      }
      case 7 /* EOF */: {
        eofInBody(p, token);
        break;
      }
      default:
    }
  }
  function whitespaceCharacterInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertCharacters(token);
  }
  function characterInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertCharacters(token);
    p.framesetOk = false;
  }
  function htmlStartTagInBody(p, token) {
    if (p.openElements.tmplCount === 0) {
      p.treeAdapter.adoptAttributes(p.openElements.items[0], token.attrs);
    }
  }
  function bodyStartTagInBody(p, token) {
    const bodyElement = p.openElements.tryPeekProperlyNestedBodyElement();
    if (bodyElement && p.openElements.tmplCount === 0) {
      p.framesetOk = false;
      p.treeAdapter.adoptAttributes(bodyElement, token.attrs);
    }
  }
  function framesetStartTagInBody(p, token) {
    const bodyElement = p.openElements.tryPeekProperlyNestedBodyElement();
    if (p.framesetOk && bodyElement) {
      p.treeAdapter.detachNode(bodyElement);
      p.openElements.popAllUpToHtmlElement();
      p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
      p.insertionMode = 19 /* IN_FRAMESET */;
    }
  }
  function addressStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function numberedHeaderStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    if (p.openElements.currentTagId !== void 0 && NUMBERED_HEADERS.has(p.openElements.currentTagId)) {
      p.openElements.pop();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function preStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.skipNextNewLine = true;
    p.framesetOk = false;
  }
  function formStartTagInBody(p, token) {
    const inTemplate = p.openElements.tmplCount > 0;
    if (!p.formElement || inTemplate) {
      if (p.openElements.hasInButtonScope(83 /* P */)) {
        p._closePElement();
      }
      p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
      if (!inTemplate) {
        p.formElement = p.openElements.current;
      }
    }
  }
  function listItemStartTagInBody(p, token) {
    p.framesetOk = false;
    const tn = token.tagID;
    for (let i = p.openElements.stackTop; i >= 0; i--) {
      const elementId = p.openElements.tagIDs[i];
      if (tn === 59 /* LI */ && elementId === 59 /* LI */ || (tn === 22 /* DD */ || tn === 29 /* DT */) && (elementId === 22 /* DD */ || elementId === 29 /* DT */)) {
        p.openElements.generateImpliedEndTagsWithExclusion(elementId);
        p.openElements.popUntilTagNamePopped(elementId);
        break;
      }
      if (elementId !== 2 /* ADDRESS */ && elementId !== 27 /* DIV */ && elementId !== 83 /* P */ && p._isSpecialElement(p.openElements.items[i], elementId)) {
        break;
      }
    }
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function plaintextStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.tokenizer.state = TokenizerMode.PLAINTEXT;
  }
  function buttonStartTagInBody(p, token) {
    if (p.openElements.hasInScope(16 /* BUTTON */)) {
      p.openElements.generateImpliedEndTags();
      p.openElements.popUntilTagNamePopped(16 /* BUTTON */);
    }
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.framesetOk = false;
  }
  function aStartTagInBody(p, token) {
    const activeElementEntry = p.activeFormattingElements.getElementEntryInScopeWithTagName("a" /* A */);
    if (activeElementEntry) {
      callAdoptionAgency(p, token);
      p.openElements.remove(activeElementEntry.element);
      p.activeFormattingElements.removeEntry(activeElementEntry);
    }
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.activeFormattingElements.pushElement(p.openElements.current, token);
  }
  function bStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.activeFormattingElements.pushElement(p.openElements.current, token);
  }
  function nobrStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    if (p.openElements.hasInScope(75 /* NOBR */)) {
      callAdoptionAgency(p, token);
      p._reconstructActiveFormattingElements();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.activeFormattingElements.pushElement(p.openElements.current, token);
  }
  function appletStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.activeFormattingElements.insertMarker();
    p.framesetOk = false;
  }
  function tableStartTagInBody(p, token) {
    if (p.treeAdapter.getDocumentMode(p.document) !== "quirks" /* QUIRKS */ && p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.framesetOk = false;
    p.insertionMode = 8 /* IN_TABLE */;
  }
  function areaStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.framesetOk = false;
    token.ackSelfClosing = true;
  }
  function isHiddenInput(token) {
    const inputType = getTokenAttr(token, "type" /* TYPE */);
    return inputType != null && inputType.toLowerCase() === HIDDEN_INPUT_TYPE;
  }
  function inputStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    if (!isHiddenInput(token)) {
      p.framesetOk = false;
    }
    token.ackSelfClosing = true;
  }
  function paramStartTagInBody(p, token) {
    p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    token.ackSelfClosing = true;
  }
  function hrStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.framesetOk = false;
    token.ackSelfClosing = true;
  }
  function imageStartTagInBody(p, token) {
    token.tagName = "img" /* IMG */;
    token.tagID = 53 /* IMG */;
    areaStartTagInBody(p, token);
  }
  function textareaStartTagInBody(p, token) {
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.skipNextNewLine = true;
    p.tokenizer.state = TokenizerMode.RCDATA;
    p.originalInsertionMode = p.insertionMode;
    p.framesetOk = false;
    p.insertionMode = 7 /* TEXT */;
  }
  function xmpStartTagInBody(p, token) {
    if (p.openElements.hasInButtonScope(83 /* P */)) {
      p._closePElement();
    }
    p._reconstructActiveFormattingElements();
    p.framesetOk = false;
    p._switchToTextParsing(token, TokenizerMode.RAWTEXT);
  }
  function iframeStartTagInBody(p, token) {
    p.framesetOk = false;
    p._switchToTextParsing(token, TokenizerMode.RAWTEXT);
  }
  function rawTextStartTagInBody(p, token) {
    p._switchToTextParsing(token, TokenizerMode.RAWTEXT);
  }
  function selectStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.framesetOk = false;
    p.insertionMode = p.insertionMode === 8 /* IN_TABLE */ || p.insertionMode === 10 /* IN_CAPTION */ || p.insertionMode === 12 /* IN_TABLE_BODY */ || p.insertionMode === 13 /* IN_ROW */ || p.insertionMode === 14 /* IN_CELL */ ? 16 /* IN_SELECT_IN_TABLE */ : 15 /* IN_SELECT */;
  }
  function optgroupStartTagInBody(p, token) {
    if (p.openElements.currentTagId === 82 /* OPTION */) {
      p.openElements.pop();
    }
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function rbStartTagInBody(p, token) {
    if (p.openElements.hasInScope(91 /* RUBY */)) {
      p.openElements.generateImpliedEndTags();
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function rtStartTagInBody(p, token) {
    if (p.openElements.hasInScope(91 /* RUBY */)) {
      p.openElements.generateImpliedEndTagsWithExclusion(90 /* RTC */);
    }
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function mathStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    adjustTokenMathMLAttrs(token);
    adjustTokenXMLAttrs(token);
    if (token.selfClosing) {
      p._appendElement(token, "http://www.w3.org/1998/Math/MathML" /* MATHML */);
    } else {
      p._insertElement(token, "http://www.w3.org/1998/Math/MathML" /* MATHML */);
    }
    token.ackSelfClosing = true;
  }
  function svgStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    adjustTokenSVGAttrs(token);
    adjustTokenXMLAttrs(token);
    if (token.selfClosing) {
      p._appendElement(token, "http://www.w3.org/2000/svg" /* SVG */);
    } else {
      p._insertElement(token, "http://www.w3.org/2000/svg" /* SVG */);
    }
    token.ackSelfClosing = true;
  }
  function genericStartTagInBody(p, token) {
    p._reconstructActiveFormattingElements();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
  }
  function startTagInBody(p, token) {
    switch (token.tagID) {
      case 52 /* I */:
      case 92 /* S */:
      case 8 /* B */:
      case 118 /* U */:
      case 30 /* EM */:
      case 117 /* TT */:
      case 12 /* BIG */:
      case 19 /* CODE */:
      case 35 /* FONT */:
      case 98 /* SMALL */:
      case 100 /* STRIKE */:
      case 101 /* STRONG */: {
        bStartTagInBody(p, token);
        break;
      }
      case 1 /* A */: {
        aStartTagInBody(p, token);
        break;
      }
      case 41 /* H1 */:
      case 42 /* H2 */:
      case 43 /* H3 */:
      case 44 /* H4 */:
      case 45 /* H5 */:
      case 46 /* H6 */: {
        numberedHeaderStartTagInBody(p, token);
        break;
      }
      case 83 /* P */:
      case 28 /* DL */:
      case 80 /* OL */:
      case 119 /* UL */:
      case 27 /* DIV */:
      case 26 /* DIR */:
      case 74 /* NAV */:
      case 62 /* MAIN */:
      case 66 /* MENU */:
      case 7 /* ASIDE */:
      case 18 /* CENTER */:
      case 34 /* FIGURE */:
      case 36 /* FOOTER */:
      case 48 /* HEADER */:
      case 49 /* HGROUP */:
      case 25 /* DIALOG */:
      case 24 /* DETAILS */:
      case 2 /* ADDRESS */:
      case 6 /* ARTICLE */:
      case 94 /* SEARCH */:
      case 95 /* SECTION */:
      case 104 /* SUMMARY */:
      case 32 /* FIELDSET */:
      case 13 /* BLOCKQUOTE */:
      case 33 /* FIGCAPTION */: {
        addressStartTagInBody(p, token);
        break;
      }
      case 59 /* LI */:
      case 22 /* DD */:
      case 29 /* DT */: {
        listItemStartTagInBody(p, token);
        break;
      }
      case 15 /* BR */:
      case 53 /* IMG */:
      case 122 /* WBR */:
      case 5 /* AREA */:
      case 31 /* EMBED */:
      case 57 /* KEYGEN */: {
        areaStartTagInBody(p, token);
        break;
      }
      case 50 /* HR */: {
        hrStartTagInBody(p, token);
        break;
      }
      case 87 /* RB */:
      case 90 /* RTC */: {
        rbStartTagInBody(p, token);
        break;
      }
      case 89 /* RT */:
      case 88 /* RP */: {
        rtStartTagInBody(p, token);
        break;
      }
      case 86 /* PRE */:
      case 61 /* LISTING */: {
        preStartTagInBody(p, token);
        break;
      }
      case 123 /* XMP */: {
        xmpStartTagInBody(p, token);
        break;
      }
      case 120 /* SVG */: {
        svgStartTagInBody(p, token);
        break;
      }
      case 51 /* HTML */: {
        htmlStartTagInBody(p, token);
        break;
      }
      case 9 /* BASE */:
      case 60 /* LINK */:
      case 67 /* META */:
      case 102 /* STYLE */:
      case 114 /* TITLE */:
      case 93 /* SCRIPT */:
      case 11 /* BGSOUND */:
      case 10 /* BASEFONT */:
      case 108 /* TEMPLATE */: {
        startTagInHead(p, token);
        break;
      }
      case 14 /* BODY */: {
        bodyStartTagInBody(p, token);
        break;
      }
      case 38 /* FORM */: {
        formStartTagInBody(p, token);
        break;
      }
      case 75 /* NOBR */: {
        nobrStartTagInBody(p, token);
        break;
      }
      case 65 /* MATH */: {
        mathStartTagInBody(p, token);
        break;
      }
      case 106 /* TABLE */: {
        tableStartTagInBody(p, token);
        break;
      }
      case 55 /* INPUT */: {
        inputStartTagInBody(p, token);
        break;
      }
      case 84 /* PARAM */:
      case 116 /* TRACK */:
      case 97 /* SOURCE */: {
        paramStartTagInBody(p, token);
        break;
      }
      case 54 /* IMAGE */: {
        imageStartTagInBody(p, token);
        break;
      }
      case 16 /* BUTTON */: {
        buttonStartTagInBody(p, token);
        break;
      }
      case 4 /* APPLET */:
      case 79 /* OBJECT */:
      case 64 /* MARQUEE */: {
        appletStartTagInBody(p, token);
        break;
      }
      case 56 /* IFRAME */: {
        iframeStartTagInBody(p, token);
        break;
      }
      case 96 /* SELECT */: {
        selectStartTagInBody(p, token);
        break;
      }
      case 82 /* OPTION */:
      case 81 /* OPTGROUP */: {
        optgroupStartTagInBody(p, token);
        break;
      }
      case 77 /* NOEMBED */:
      case 76 /* NOFRAMES */: {
        rawTextStartTagInBody(p, token);
        break;
      }
      case 40 /* FRAMESET */: {
        framesetStartTagInBody(p, token);
        break;
      }
      case 109 /* TEXTAREA */: {
        textareaStartTagInBody(p, token);
        break;
      }
      case 78 /* NOSCRIPT */: {
        if (p.options.scriptingEnabled) {
          rawTextStartTagInBody(p, token);
        } else {
          genericStartTagInBody(p, token);
        }
        break;
      }
      case 85 /* PLAINTEXT */: {
        plaintextStartTagInBody(p, token);
        break;
      }
      case 20 /* COL */:
      case 112 /* TH */:
      case 111 /* TD */:
      case 115 /* TR */:
      case 47 /* HEAD */:
      case 39 /* FRAME */:
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */:
      case 17 /* CAPTION */:
      case 21 /* COLGROUP */: {
        break;
      }
      default: {
        genericStartTagInBody(p, token);
      }
    }
  }
  function bodyEndTagInBody(p, token) {
    if (p.openElements.hasInScope(14 /* BODY */)) {
      p.insertionMode = 18 /* AFTER_BODY */;
      if (p.options.sourceCodeLocationInfo) {
        const bodyElement = p.openElements.tryPeekProperlyNestedBodyElement();
        if (bodyElement) {
          p._setEndLocation(bodyElement, token);
        }
      }
    }
  }
  function htmlEndTagInBody(p, token) {
    if (p.openElements.hasInScope(14 /* BODY */)) {
      p.insertionMode = 18 /* AFTER_BODY */;
      endTagAfterBody(p, token);
    }
  }
  function addressEndTagInBody(p, token) {
    const tn = token.tagID;
    if (p.openElements.hasInScope(tn)) {
      p.openElements.generateImpliedEndTags();
      p.openElements.popUntilTagNamePopped(tn);
    }
  }
  function formEndTagInBody(p) {
    const inTemplate = p.openElements.tmplCount > 0;
    const { formElement } = p;
    if (!inTemplate) {
      p.formElement = null;
    }
    if ((formElement || inTemplate) && p.openElements.hasInScope(38 /* FORM */)) {
      p.openElements.generateImpliedEndTags();
      if (inTemplate) {
        p.openElements.popUntilTagNamePopped(38 /* FORM */);
      } else if (formElement) {
        p.openElements.remove(formElement);
      }
    }
  }
  function pEndTagInBody(p) {
    if (!p.openElements.hasInButtonScope(83 /* P */)) {
      p._insertFakeElement("p" /* P */, 83 /* P */);
    }
    p._closePElement();
  }
  function liEndTagInBody(p) {
    if (p.openElements.hasInListItemScope(59 /* LI */)) {
      p.openElements.generateImpliedEndTagsWithExclusion(59 /* LI */);
      p.openElements.popUntilTagNamePopped(59 /* LI */);
    }
  }
  function ddEndTagInBody(p, token) {
    const tn = token.tagID;
    if (p.openElements.hasInScope(tn)) {
      p.openElements.generateImpliedEndTagsWithExclusion(tn);
      p.openElements.popUntilTagNamePopped(tn);
    }
  }
  function numberedHeaderEndTagInBody(p) {
    if (p.openElements.hasNumberedHeaderInScope()) {
      p.openElements.generateImpliedEndTags();
      p.openElements.popUntilNumberedHeaderPopped();
    }
  }
  function appletEndTagInBody(p, token) {
    const tn = token.tagID;
    if (p.openElements.hasInScope(tn)) {
      p.openElements.generateImpliedEndTags();
      p.openElements.popUntilTagNamePopped(tn);
      p.activeFormattingElements.clearToLastMarker();
    }
  }
  function brEndTagInBody(p) {
    p._reconstructActiveFormattingElements();
    p._insertFakeElement("br" /* BR */, 15 /* BR */);
    p.openElements.pop();
    p.framesetOk = false;
  }
  function genericEndTagInBody(p, token) {
    const tn = token.tagName;
    const tid = token.tagID;
    for (let i = p.openElements.stackTop; i > 0; i--) {
      const element = p.openElements.items[i];
      const elementId = p.openElements.tagIDs[i];
      if (tid === elementId && p.treeAdapter.getNamespaceURI(element) === "http://www.w3.org/1999/xhtml" /* HTML */ && (tid !== 0 /* UNKNOWN */ || p.treeAdapter.getTagName(element) === tn)) {
        p.openElements.generateImpliedEndTagsWithExclusion(tid);
        if (p.openElements.stackTop >= i) p.openElements.shortenToLength(i);
        break;
      }
      if (p._isSpecialElement(element, elementId)) {
        break;
      }
    }
  }
  function endTagInBody(p, token) {
    switch (token.tagID) {
      case 1 /* A */:
      case 8 /* B */:
      case 52 /* I */:
      case 92 /* S */:
      case 118 /* U */:
      case 30 /* EM */:
      case 117 /* TT */:
      case 12 /* BIG */:
      case 19 /* CODE */:
      case 35 /* FONT */:
      case 75 /* NOBR */:
      case 98 /* SMALL */:
      case 100 /* STRIKE */:
      case 101 /* STRONG */: {
        callAdoptionAgency(p, token);
        break;
      }
      case 83 /* P */: {
        pEndTagInBody(p);
        break;
      }
      case 28 /* DL */:
      case 119 /* UL */:
      case 80 /* OL */:
      case 26 /* DIR */:
      case 27 /* DIV */:
      case 74 /* NAV */:
      case 86 /* PRE */:
      case 62 /* MAIN */:
      case 66 /* MENU */:
      case 7 /* ASIDE */:
      case 16 /* BUTTON */:
      case 18 /* CENTER */:
      case 34 /* FIGURE */:
      case 36 /* FOOTER */:
      case 48 /* HEADER */:
      case 49 /* HGROUP */:
      case 25 /* DIALOG */:
      case 2 /* ADDRESS */:
      case 6 /* ARTICLE */:
      case 24 /* DETAILS */:
      case 94 /* SEARCH */:
      case 95 /* SECTION */:
      case 104 /* SUMMARY */:
      case 61 /* LISTING */:
      case 32 /* FIELDSET */:
      case 13 /* BLOCKQUOTE */:
      case 33 /* FIGCAPTION */: {
        addressEndTagInBody(p, token);
        break;
      }
      case 59 /* LI */: {
        liEndTagInBody(p);
        break;
      }
      case 22 /* DD */:
      case 29 /* DT */: {
        ddEndTagInBody(p, token);
        break;
      }
      case 41 /* H1 */:
      case 42 /* H2 */:
      case 43 /* H3 */:
      case 44 /* H4 */:
      case 45 /* H5 */:
      case 46 /* H6 */: {
        numberedHeaderEndTagInBody(p);
        break;
      }
      case 15 /* BR */: {
        brEndTagInBody(p);
        break;
      }
      case 14 /* BODY */: {
        bodyEndTagInBody(p, token);
        break;
      }
      case 51 /* HTML */: {
        htmlEndTagInBody(p, token);
        break;
      }
      case 38 /* FORM */: {
        formEndTagInBody(p);
        break;
      }
      case 4 /* APPLET */:
      case 79 /* OBJECT */:
      case 64 /* MARQUEE */: {
        appletEndTagInBody(p, token);
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      default: {
        genericEndTagInBody(p, token);
      }
    }
  }
  function eofInBody(p, token) {
    if (p.tmplInsertionModeStack.length > 0) {
      eofInTemplate(p, token);
    } else {
      stopParsing(p, token);
    }
  }
  function endTagInText(p, token) {
    if (token.tagID === 93 /* SCRIPT */) {
      p.scriptHandler?.(p.openElements.current);
    }
    p.openElements.pop();
    p.insertionMode = p.originalInsertionMode;
  }
  function eofInText(p, token) {
    p._err(token, "eof-in-element-that-can-contain-only-text" /* eofInElementThatCanContainOnlyText */);
    p.openElements.pop();
    p.insertionMode = p.originalInsertionMode;
    p.onEof(token);
  }
  function characterInTable(p, token) {
    if (p.openElements.currentTagId !== void 0 && TABLE_STRUCTURE_TAGS.has(p.openElements.currentTagId)) {
      p.pendingCharacterTokens.length = 0;
      p.hasNonWhitespacePendingCharacterToken = false;
      p.originalInsertionMode = p.insertionMode;
      p.insertionMode = 9 /* IN_TABLE_TEXT */;
      switch (token.type) {
        case 0 /* CHARACTER */: {
          characterInTableText(p, token);
          break;
        }
        case 2 /* WHITESPACE_CHARACTER */: {
          whitespaceCharacterInTableText(p, token);
          break;
        }
      }
    } else {
      tokenInTable(p, token);
    }
  }
  function captionStartTagInTable(p, token) {
    p.openElements.clearBackToTableContext();
    p.activeFormattingElements.insertMarker();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.insertionMode = 10 /* IN_CAPTION */;
  }
  function colgroupStartTagInTable(p, token) {
    p.openElements.clearBackToTableContext();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.insertionMode = 11 /* IN_COLUMN_GROUP */;
  }
  function colStartTagInTable(p, token) {
    p.openElements.clearBackToTableContext();
    p._insertFakeElement("colgroup" /* COLGROUP */, 21 /* COLGROUP */);
    p.insertionMode = 11 /* IN_COLUMN_GROUP */;
    startTagInColumnGroup(p, token);
  }
  function tbodyStartTagInTable(p, token) {
    p.openElements.clearBackToTableContext();
    p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    p.insertionMode = 12 /* IN_TABLE_BODY */;
  }
  function tdStartTagInTable(p, token) {
    p.openElements.clearBackToTableContext();
    p._insertFakeElement("tbody" /* TBODY */, 107 /* TBODY */);
    p.insertionMode = 12 /* IN_TABLE_BODY */;
    startTagInTableBody(p, token);
  }
  function tableStartTagInTable(p, token) {
    if (p.openElements.hasInTableScope(106 /* TABLE */)) {
      p.openElements.popUntilTagNamePopped(106 /* TABLE */);
      p._resetInsertionMode();
      p._processStartTag(token);
    }
  }
  function inputStartTagInTable(p, token) {
    if (isHiddenInput(token)) {
      p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
    } else {
      tokenInTable(p, token);
    }
    token.ackSelfClosing = true;
  }
  function formStartTagInTable(p, token) {
    if (!p.formElement && p.openElements.tmplCount === 0) {
      p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
      p.formElement = p.openElements.current;
      p.openElements.pop();
    }
  }
  function startTagInTable(p, token) {
    switch (token.tagID) {
      case 111 /* TD */:
      case 112 /* TH */:
      case 115 /* TR */: {
        tdStartTagInTable(p, token);
        break;
      }
      case 102 /* STYLE */:
      case 93 /* SCRIPT */:
      case 108 /* TEMPLATE */: {
        startTagInHead(p, token);
        break;
      }
      case 20 /* COL */: {
        colStartTagInTable(p, token);
        break;
      }
      case 38 /* FORM */: {
        formStartTagInTable(p, token);
        break;
      }
      case 106 /* TABLE */: {
        tableStartTagInTable(p, token);
        break;
      }
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */: {
        tbodyStartTagInTable(p, token);
        break;
      }
      case 55 /* INPUT */: {
        inputStartTagInTable(p, token);
        break;
      }
      case 17 /* CAPTION */: {
        captionStartTagInTable(p, token);
        break;
      }
      case 21 /* COLGROUP */: {
        colgroupStartTagInTable(p, token);
        break;
      }
      default: {
        tokenInTable(p, token);
      }
    }
  }
  function endTagInTable(p, token) {
    switch (token.tagID) {
      case 106 /* TABLE */: {
        if (p.openElements.hasInTableScope(106 /* TABLE */)) {
          p.openElements.popUntilTagNamePopped(106 /* TABLE */);
          p._resetInsertionMode();
        }
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      case 14 /* BODY */:
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 51 /* HTML */:
      case 107 /* TBODY */:
      case 111 /* TD */:
      case 110 /* TFOOT */:
      case 112 /* TH */:
      case 113 /* THEAD */:
      case 115 /* TR */: {
        break;
      }
      default: {
        tokenInTable(p, token);
      }
    }
  }
  function tokenInTable(p, token) {
    const savedFosterParentingState = p.fosterParentingEnabled;
    p.fosterParentingEnabled = true;
    modeInBody(p, token);
    p.fosterParentingEnabled = savedFosterParentingState;
  }
  function whitespaceCharacterInTableText(p, token) {
    p.pendingCharacterTokens.push(token);
  }
  function characterInTableText(p, token) {
    p.pendingCharacterTokens.push(token);
    p.hasNonWhitespacePendingCharacterToken = true;
  }
  function tokenInTableText(p, token) {
    let i = 0;
    if (p.hasNonWhitespacePendingCharacterToken) {
      for (; i < p.pendingCharacterTokens.length; i++) {
        tokenInTable(p, p.pendingCharacterTokens[i]);
      }
    } else {
      for (; i < p.pendingCharacterTokens.length; i++) {
        p._insertCharacters(p.pendingCharacterTokens[i]);
      }
    }
    p.insertionMode = p.originalInsertionMode;
    p._processToken(token);
  }
  var TABLE_VOID_ELEMENTS = /* @__PURE__ */ new Set([17 /* CAPTION */, 20 /* COL */, 21 /* COLGROUP */, 107 /* TBODY */, 111 /* TD */, 110 /* TFOOT */, 112 /* TH */, 113 /* THEAD */, 115 /* TR */]);
  function startTagInCaption(p, token) {
    const tn = token.tagID;
    if (TABLE_VOID_ELEMENTS.has(tn)) {
      if (p.openElements.hasInTableScope(17 /* CAPTION */)) {
        p.openElements.generateImpliedEndTags();
        p.openElements.popUntilTagNamePopped(17 /* CAPTION */);
        p.activeFormattingElements.clearToLastMarker();
        p.insertionMode = 8 /* IN_TABLE */;
        startTagInTable(p, token);
      }
    } else {
      startTagInBody(p, token);
    }
  }
  function endTagInCaption(p, token) {
    const tn = token.tagID;
    switch (tn) {
      case 17 /* CAPTION */:
      case 106 /* TABLE */: {
        if (p.openElements.hasInTableScope(17 /* CAPTION */)) {
          p.openElements.generateImpliedEndTags();
          p.openElements.popUntilTagNamePopped(17 /* CAPTION */);
          p.activeFormattingElements.clearToLastMarker();
          p.insertionMode = 8 /* IN_TABLE */;
          if (tn === 106 /* TABLE */) {
            endTagInTable(p, token);
          }
        }
        break;
      }
      case 14 /* BODY */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 51 /* HTML */:
      case 107 /* TBODY */:
      case 111 /* TD */:
      case 110 /* TFOOT */:
      case 112 /* TH */:
      case 113 /* THEAD */:
      case 115 /* TR */: {
        break;
      }
      default: {
        endTagInBody(p, token);
      }
    }
  }
  function startTagInColumnGroup(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 20 /* COL */: {
        p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        token.ackSelfClosing = true;
        break;
      }
      case 108 /* TEMPLATE */: {
        startTagInHead(p, token);
        break;
      }
      default: {
        tokenInColumnGroup(p, token);
      }
    }
  }
  function endTagInColumnGroup(p, token) {
    switch (token.tagID) {
      case 21 /* COLGROUP */: {
        if (p.openElements.currentTagId === 21 /* COLGROUP */) {
          p.openElements.pop();
          p.insertionMode = 8 /* IN_TABLE */;
        }
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      case 20 /* COL */: {
        break;
      }
      default: {
        tokenInColumnGroup(p, token);
      }
    }
  }
  function tokenInColumnGroup(p, token) {
    if (p.openElements.currentTagId === 21 /* COLGROUP */) {
      p.openElements.pop();
      p.insertionMode = 8 /* IN_TABLE */;
      p._processToken(token);
    }
  }
  function startTagInTableBody(p, token) {
    switch (token.tagID) {
      case 115 /* TR */: {
        p.openElements.clearBackToTableBodyContext();
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        p.insertionMode = 13 /* IN_ROW */;
        break;
      }
      case 112 /* TH */:
      case 111 /* TD */: {
        p.openElements.clearBackToTableBodyContext();
        p._insertFakeElement("tr" /* TR */, 115 /* TR */);
        p.insertionMode = 13 /* IN_ROW */;
        startTagInRow(p, token);
        break;
      }
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */: {
        if (p.openElements.hasTableBodyContextInTableScope()) {
          p.openElements.clearBackToTableBodyContext();
          p.openElements.pop();
          p.insertionMode = 8 /* IN_TABLE */;
          startTagInTable(p, token);
        }
        break;
      }
      default: {
        startTagInTable(p, token);
      }
    }
  }
  function endTagInTableBody(p, token) {
    const tn = token.tagID;
    switch (token.tagID) {
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */: {
        if (p.openElements.hasInTableScope(tn)) {
          p.openElements.clearBackToTableBodyContext();
          p.openElements.pop();
          p.insertionMode = 8 /* IN_TABLE */;
        }
        break;
      }
      case 106 /* TABLE */: {
        if (p.openElements.hasTableBodyContextInTableScope()) {
          p.openElements.clearBackToTableBodyContext();
          p.openElements.pop();
          p.insertionMode = 8 /* IN_TABLE */;
          endTagInTable(p, token);
        }
        break;
      }
      case 14 /* BODY */:
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 51 /* HTML */:
      case 111 /* TD */:
      case 112 /* TH */:
      case 115 /* TR */: {
        break;
      }
      default: {
        endTagInTable(p, token);
      }
    }
  }
  function startTagInRow(p, token) {
    switch (token.tagID) {
      case 112 /* TH */:
      case 111 /* TD */: {
        p.openElements.clearBackToTableRowContext();
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        p.insertionMode = 14 /* IN_CELL */;
        p.activeFormattingElements.insertMarker();
        break;
      }
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */:
      case 115 /* TR */: {
        if (p.openElements.hasInTableScope(115 /* TR */)) {
          p.openElements.clearBackToTableRowContext();
          p.openElements.pop();
          p.insertionMode = 12 /* IN_TABLE_BODY */;
          startTagInTableBody(p, token);
        }
        break;
      }
      default: {
        startTagInTable(p, token);
      }
    }
  }
  function endTagInRow(p, token) {
    switch (token.tagID) {
      case 115 /* TR */: {
        if (p.openElements.hasInTableScope(115 /* TR */)) {
          p.openElements.clearBackToTableRowContext();
          p.openElements.pop();
          p.insertionMode = 12 /* IN_TABLE_BODY */;
        }
        break;
      }
      case 106 /* TABLE */: {
        if (p.openElements.hasInTableScope(115 /* TR */)) {
          p.openElements.clearBackToTableRowContext();
          p.openElements.pop();
          p.insertionMode = 12 /* IN_TABLE_BODY */;
          endTagInTableBody(p, token);
        }
        break;
      }
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */: {
        if (p.openElements.hasInTableScope(token.tagID) || p.openElements.hasInTableScope(115 /* TR */)) {
          p.openElements.clearBackToTableRowContext();
          p.openElements.pop();
          p.insertionMode = 12 /* IN_TABLE_BODY */;
          endTagInTableBody(p, token);
        }
        break;
      }
      case 14 /* BODY */:
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 51 /* HTML */:
      case 111 /* TD */:
      case 112 /* TH */: {
        break;
      }
      default: {
        endTagInTable(p, token);
      }
    }
  }
  function startTagInCell(p, token) {
    const tn = token.tagID;
    if (TABLE_VOID_ELEMENTS.has(tn)) {
      if (p.openElements.hasInTableScope(111 /* TD */) || p.openElements.hasInTableScope(112 /* TH */)) {
        p._closeTableCell();
        startTagInRow(p, token);
      }
    } else {
      startTagInBody(p, token);
    }
  }
  function endTagInCell(p, token) {
    const tn = token.tagID;
    switch (tn) {
      case 111 /* TD */:
      case 112 /* TH */: {
        if (p.openElements.hasInTableScope(tn)) {
          p.openElements.generateImpliedEndTags();
          p.openElements.popUntilTagNamePopped(tn);
          p.activeFormattingElements.clearToLastMarker();
          p.insertionMode = 13 /* IN_ROW */;
        }
        break;
      }
      case 106 /* TABLE */:
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */:
      case 115 /* TR */: {
        if (p.openElements.hasInTableScope(tn)) {
          p._closeTableCell();
          endTagInRow(p, token);
        }
        break;
      }
      case 14 /* BODY */:
      case 17 /* CAPTION */:
      case 20 /* COL */:
      case 21 /* COLGROUP */:
      case 51 /* HTML */: {
        break;
      }
      default: {
        endTagInBody(p, token);
      }
    }
  }
  function startTagInSelect(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 82 /* OPTION */: {
        if (p.openElements.currentTagId === 82 /* OPTION */) {
          p.openElements.pop();
        }
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        break;
      }
      case 81 /* OPTGROUP */: {
        if (p.openElements.currentTagId === 82 /* OPTION */) {
          p.openElements.pop();
        }
        if (p.openElements.currentTagId === 81 /* OPTGROUP */) {
          p.openElements.pop();
        }
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        break;
      }
      case 50 /* HR */: {
        if (p.openElements.currentTagId === 82 /* OPTION */) {
          p.openElements.pop();
        }
        if (p.openElements.currentTagId === 81 /* OPTGROUP */) {
          p.openElements.pop();
        }
        p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        token.ackSelfClosing = true;
        break;
      }
      case 55 /* INPUT */:
      case 57 /* KEYGEN */:
      case 109 /* TEXTAREA */:
      case 96 /* SELECT */: {
        if (p.openElements.hasInSelectScope(96 /* SELECT */)) {
          p.openElements.popUntilTagNamePopped(96 /* SELECT */);
          p._resetInsertionMode();
          if (token.tagID !== 96 /* SELECT */) {
            p._processStartTag(token);
          }
        }
        break;
      }
      case 93 /* SCRIPT */:
      case 108 /* TEMPLATE */: {
        startTagInHead(p, token);
        break;
      }
      default:
    }
  }
  function endTagInSelect(p, token) {
    switch (token.tagID) {
      case 81 /* OPTGROUP */: {
        if (p.openElements.stackTop > 0 && p.openElements.currentTagId === 82 /* OPTION */ && p.openElements.tagIDs[p.openElements.stackTop - 1] === 81 /* OPTGROUP */) {
          p.openElements.pop();
        }
        if (p.openElements.currentTagId === 81 /* OPTGROUP */) {
          p.openElements.pop();
        }
        break;
      }
      case 82 /* OPTION */: {
        if (p.openElements.currentTagId === 82 /* OPTION */) {
          p.openElements.pop();
        }
        break;
      }
      case 96 /* SELECT */: {
        if (p.openElements.hasInSelectScope(96 /* SELECT */)) {
          p.openElements.popUntilTagNamePopped(96 /* SELECT */);
          p._resetInsertionMode();
        }
        break;
      }
      case 108 /* TEMPLATE */: {
        templateEndTagInHead(p, token);
        break;
      }
      default:
    }
  }
  function startTagInSelectInTable(p, token) {
    const tn = token.tagID;
    if (tn === 17 /* CAPTION */ || tn === 106 /* TABLE */ || tn === 107 /* TBODY */ || tn === 110 /* TFOOT */ || tn === 113 /* THEAD */ || tn === 115 /* TR */ || tn === 111 /* TD */ || tn === 112 /* TH */) {
      p.openElements.popUntilTagNamePopped(96 /* SELECT */);
      p._resetInsertionMode();
      p._processStartTag(token);
    } else {
      startTagInSelect(p, token);
    }
  }
  function endTagInSelectInTable(p, token) {
    const tn = token.tagID;
    if (tn === 17 /* CAPTION */ || tn === 106 /* TABLE */ || tn === 107 /* TBODY */ || tn === 110 /* TFOOT */ || tn === 113 /* THEAD */ || tn === 115 /* TR */ || tn === 111 /* TD */ || tn === 112 /* TH */) {
      if (p.openElements.hasInTableScope(tn)) {
        p.openElements.popUntilTagNamePopped(96 /* SELECT */);
        p._resetInsertionMode();
        p.onEndTag(token);
      }
    } else {
      endTagInSelect(p, token);
    }
  }
  function startTagInTemplate(p, token) {
    switch (token.tagID) {
      // First, handle tags that can start without a mode change
      case 9 /* BASE */:
      case 10 /* BASEFONT */:
      case 11 /* BGSOUND */:
      case 60 /* LINK */:
      case 67 /* META */:
      case 76 /* NOFRAMES */:
      case 93 /* SCRIPT */:
      case 102 /* STYLE */:
      case 108 /* TEMPLATE */:
      case 114 /* TITLE */: {
        startTagInHead(p, token);
        break;
      }
      // Re-process the token in the appropriate mode
      case 17 /* CAPTION */:
      case 21 /* COLGROUP */:
      case 107 /* TBODY */:
      case 110 /* TFOOT */:
      case 113 /* THEAD */: {
        p.tmplInsertionModeStack[0] = 8 /* IN_TABLE */;
        p.insertionMode = 8 /* IN_TABLE */;
        startTagInTable(p, token);
        break;
      }
      case 20 /* COL */: {
        p.tmplInsertionModeStack[0] = 11 /* IN_COLUMN_GROUP */;
        p.insertionMode = 11 /* IN_COLUMN_GROUP */;
        startTagInColumnGroup(p, token);
        break;
      }
      case 115 /* TR */: {
        p.tmplInsertionModeStack[0] = 12 /* IN_TABLE_BODY */;
        p.insertionMode = 12 /* IN_TABLE_BODY */;
        startTagInTableBody(p, token);
        break;
      }
      case 111 /* TD */:
      case 112 /* TH */: {
        p.tmplInsertionModeStack[0] = 13 /* IN_ROW */;
        p.insertionMode = 13 /* IN_ROW */;
        startTagInRow(p, token);
        break;
      }
      default: {
        p.tmplInsertionModeStack[0] = 6 /* IN_BODY */;
        p.insertionMode = 6 /* IN_BODY */;
        startTagInBody(p, token);
      }
    }
  }
  function endTagInTemplate(p, token) {
    if (token.tagID === 108 /* TEMPLATE */) {
      templateEndTagInHead(p, token);
    }
  }
  function eofInTemplate(p, token) {
    if (p.openElements.tmplCount > 0) {
      p.openElements.popUntilTagNamePopped(108 /* TEMPLATE */);
      p.activeFormattingElements.clearToLastMarker();
      p.tmplInsertionModeStack.shift();
      p._resetInsertionMode();
      p.onEof(token);
    } else {
      stopParsing(p, token);
    }
  }
  function startTagAfterBody(p, token) {
    if (token.tagID === 51 /* HTML */) {
      startTagInBody(p, token);
    } else {
      tokenAfterBody(p, token);
    }
  }
  function endTagAfterBody(p, token) {
    if (token.tagID === 51 /* HTML */) {
      if (!p.fragmentContext) {
        p.insertionMode = 21 /* AFTER_AFTER_BODY */;
      }
      if (p.options.sourceCodeLocationInfo && p.openElements.tagIDs[0] === 51 /* HTML */) {
        p._setEndLocation(p.openElements.items[0], token);
        const bodyElement = p.openElements.items[1];
        if (bodyElement && !p.treeAdapter.getNodeSourceCodeLocation(bodyElement)?.endTag) {
          p._setEndLocation(bodyElement, token);
        }
      }
    } else {
      tokenAfterBody(p, token);
    }
  }
  function tokenAfterBody(p, token) {
    p.insertionMode = 6 /* IN_BODY */;
    modeInBody(p, token);
  }
  function startTagInFrameset(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 40 /* FRAMESET */: {
        p._insertElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        break;
      }
      case 39 /* FRAME */: {
        p._appendElement(token, "http://www.w3.org/1999/xhtml" /* HTML */);
        token.ackSelfClosing = true;
        break;
      }
      case 76 /* NOFRAMES */: {
        startTagInHead(p, token);
        break;
      }
      default:
    }
  }
  function endTagInFrameset(p, token) {
    if (token.tagID === 40 /* FRAMESET */ && !p.openElements.isRootHtmlElementCurrent()) {
      p.openElements.pop();
      if (!p.fragmentContext && p.openElements.currentTagId !== 40 /* FRAMESET */) {
        p.insertionMode = 20 /* AFTER_FRAMESET */;
      }
    }
  }
  function startTagAfterFrameset(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 76 /* NOFRAMES */: {
        startTagInHead(p, token);
        break;
      }
      default:
    }
  }
  function endTagAfterFrameset(p, token) {
    if (token.tagID === 51 /* HTML */) {
      p.insertionMode = 22 /* AFTER_AFTER_FRAMESET */;
    }
  }
  function startTagAfterAfterBody(p, token) {
    if (token.tagID === 51 /* HTML */) {
      startTagInBody(p, token);
    } else {
      tokenAfterAfterBody(p, token);
    }
  }
  function tokenAfterAfterBody(p, token) {
    p.insertionMode = 6 /* IN_BODY */;
    modeInBody(p, token);
  }
  function startTagAfterAfterFrameset(p, token) {
    switch (token.tagID) {
      case 51 /* HTML */: {
        startTagInBody(p, token);
        break;
      }
      case 76 /* NOFRAMES */: {
        startTagInHead(p, token);
        break;
      }
      default:
    }
  }
  function nullCharacterInForeignContent(p, token) {
    token.chars = REPLACEMENT_CHARACTER;
    p._insertCharacters(token);
  }
  function characterInForeignContent(p, token) {
    p._insertCharacters(token);
    p.framesetOk = false;
  }
  function popUntilHtmlOrIntegrationPoint(p) {
    while (p.treeAdapter.getNamespaceURI(p.openElements.current) !== "http://www.w3.org/1999/xhtml" /* HTML */ && p.openElements.currentTagId !== void 0 && !p._isIntegrationPoint(p.openElements.currentTagId, p.openElements.current)) {
      p.openElements.pop();
    }
  }
  function startTagInForeignContent(p, token) {
    if (causesExit(token)) {
      popUntilHtmlOrIntegrationPoint(p);
      p._startTagOutsideForeignContent(token);
    } else {
      const current = p._getAdjustedCurrentElement();
      const currentNs = p.treeAdapter.getNamespaceURI(current);
      if (currentNs === "http://www.w3.org/1998/Math/MathML" /* MATHML */) {
        adjustTokenMathMLAttrs(token);
      } else if (currentNs === "http://www.w3.org/2000/svg" /* SVG */) {
        adjustTokenSVGTagName(token);
        adjustTokenSVGAttrs(token);
      }
      adjustTokenXMLAttrs(token);
      if (token.selfClosing) {
        p._appendElement(token, currentNs);
      } else {
        p._insertElement(token, currentNs);
      }
      token.ackSelfClosing = true;
    }
  }
  function endTagInForeignContent(p, token) {
    if (token.tagID === 83 /* P */ || token.tagID === 15 /* BR */) {
      popUntilHtmlOrIntegrationPoint(p);
      p._endTagOutsideForeignContent(token);
      return;
    }
    for (let i = p.openElements.stackTop; i > 0; i--) {
      const element = p.openElements.items[i];
      if (p.treeAdapter.getNamespaceURI(element) === "http://www.w3.org/1999/xhtml" /* HTML */) {
        p._endTagOutsideForeignContent(token);
        break;
      }
      const tagName = p.treeAdapter.getTagName(element);
      if (tagName.toLowerCase() === token.tagName) {
        token.tagName = tagName;
        p.openElements.shortenToLength(i);
        break;
      }
    }
  }

  // node_modules/entities/dist/escape.js
  function getEscape(char) {
    return char === 34 ? "&quot;" : char === 38 ? "&amp;" : char === 39 ? "&apos;" : char === 60 ? "&lt;" : char === 62 ? "&gt;" : "&nbsp;";
  }
  function escapeWithRegex(re, data) {
    re.lastIndex = 0;
    if (!re.test(data))
      return data;
    let out = "";
    let last = 0;
    do {
      const index = re.lastIndex - 1;
      if (last !== index)
        out += data.substring(last, index);
      const char = data.charCodeAt(index);
      out += getEscape(char);
      last = index + 1;
    } while (re.test(data));
    return out + data.substring(last);
  }
  var attributeEscapeRegex = /["&\u{A0}]/gu;
  function escapeAttribute(data) {
    return escapeWithRegex(attributeEscapeRegex, data);
  }
  var textEscapeRegex = /[&<>\u{A0}]/gu;
  function escapeText(data) {
    return escapeWithRegex(textEscapeRegex, data);
  }

  // src/serializer/index.ts
  var VOID_ELEMENTS = /* @__PURE__ */ new Set([
    "area" /* AREA */,
    "base" /* BASE */,
    "basefont" /* BASEFONT */,
    "bgsound" /* BGSOUND */,
    "br" /* BR */,
    "col" /* COL */,
    "embed" /* EMBED */,
    "frame" /* FRAME */,
    "hr" /* HR */,
    "img" /* IMG */,
    "input" /* INPUT */,
    "keygen" /* KEYGEN */,
    "link" /* LINK */,
    "meta" /* META */,
    "param" /* PARAM */,
    "source" /* SOURCE */,
    "track" /* TRACK */,
    "wbr" /* WBR */
  ]);
  function isVoidElement(node, options) {
    return options.treeAdapter.isElementNode(node) && options.treeAdapter.getNamespaceURI(node) === "http://www.w3.org/1999/xhtml" /* HTML */ && VOID_ELEMENTS.has(options.treeAdapter.getTagName(node));
  }
  var defaultOpts = { treeAdapter: defaultTreeAdapter, scriptingEnabled: true };
  function serialize(node, options) {
    const opts = { ...defaultOpts, ...options };
    if (isVoidElement(node, opts)) {
      return "";
    }
    return serializeChildNodes(node, opts);
  }
  function serializeOuter(node, options) {
    const opts = { ...defaultOpts, ...options };
    return serializeNode(node, opts);
  }
  function serializeChildNodes(parentNode, options) {
    let html = "";
    const container = options.treeAdapter.isElementNode(parentNode) && options.treeAdapter.getTagName(parentNode) === "template" /* TEMPLATE */ && options.treeAdapter.getNamespaceURI(parentNode) === "http://www.w3.org/1999/xhtml" /* HTML */ ? options.treeAdapter.getTemplateContent(parentNode) : parentNode;
    const childNodes = options.treeAdapter.getChildNodes(container);
    if (childNodes) {
      for (const currentNode of childNodes) {
        html += serializeNode(currentNode, options);
      }
    }
    return html;
  }
  function serializeNode(node, options) {
    if (options.treeAdapter.isElementNode(node)) {
      return serializeElement(node, options);
    }
    if (options.treeAdapter.isTextNode(node)) {
      return serializeTextNode(node, options);
    }
    if (options.treeAdapter.isCommentNode(node)) {
      return serializeCommentNode(node, options);
    }
    if (options.treeAdapter.isDocumentTypeNode(node)) {
      return serializeDocumentTypeNode(node, options);
    }
    return "";
  }
  function serializeElement(node, options) {
    const tn = options.treeAdapter.getTagName(node);
    return `<${tn}${serializeAttributes(node, options)}>${isVoidElement(node, options) ? "" : `${serializeChildNodes(node, options)}</${tn}>`}`;
  }
  function serializeAttributes(node, { treeAdapter }) {
    let html = "";
    for (const attr of treeAdapter.getAttrList(node)) {
      html += " ";
      if (attr.namespace) {
        switch (attr.namespace) {
          case "http://www.w3.org/XML/1998/namespace" /* XML */: {
            html += `xml:${attr.name}`;
            break;
          }
          case "http://www.w3.org/2000/xmlns/" /* XMLNS */: {
            if (attr.name !== "xmlns") {
              html += "xmlns:";
            }
            html += attr.name;
            break;
          }
          case "http://www.w3.org/1999/xlink" /* XLINK */: {
            html += `xlink:${attr.name}`;
            break;
          }
          default: {
            html += `${attr.prefix}:${attr.name}`;
          }
        }
      } else {
        html += attr.name;
      }
      html += `="${escapeAttribute(attr.value)}"`;
    }
    return html;
  }
  function serializeTextNode(node, options) {
    const { treeAdapter } = options;
    const content = treeAdapter.getTextNodeContent(node);
    const parent = treeAdapter.getParentNode(node);
    const parentTn = parent && treeAdapter.isElementNode(parent) && treeAdapter.getTagName(parent);
    return parentTn && treeAdapter.getNamespaceURI(parent) === "http://www.w3.org/1999/xhtml" /* HTML */ && hasUnescapedText(parentTn, options.scriptingEnabled) ? content : escapeText(content);
  }
  function serializeCommentNode(node, { treeAdapter }) {
    return `<!--${treeAdapter.getCommentNodeContent(node)}-->`;
  }
  function serializeDocumentTypeNode(node, { treeAdapter }) {
    return `<!DOCTYPE ${treeAdapter.getDocumentTypeNodeName(node)}>`;
  }

  // src/index.ts
  function parse(html, options) {
    return Parser.parse(html, options);
  }
  function parseFragment(fragmentContext, html, options) {
    if (typeof fragmentContext === "string") {
      options = html;
      html = fragmentContext;
      fragmentContext = null;
    }
    const parser = Parser.getFragmentParser(fragmentContext, options);
    parser.tokenizer.write(html, true);
    return parser.getFragment();
  }
  return __toCommonJS(index_exports);
})();

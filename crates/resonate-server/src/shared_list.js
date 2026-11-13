"use strict";

const PROTOCOL_VERSION = @VERSION@;
const LARGEST_OPENED = @LARGEST@;
const ROWS_AT_MOST = @ROWS@;
const ENTRIES_KEPT_AT_MOST = @ENTRIES@;
const ANSWERED_WITHIN_MS = 20000;
const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SALT_FIRST_TO_CLAIM = "resonate-shared/1";
const SALT_COMMITTED = "resonate-shared/2";
const COMMITMENT_SALT = "resonate-committed/1";
const COMMITTED_MARK = "RSc1";
const NONCE_BYTES = 12;
const TAG_BYTES = 16;
const ADDED = 0;
const MOVED = 1;
const REMOVED = 2;
const RENAMED = 3;
const VIEW = 8;
const WELCOME = 0;
const SNAPSHOT = 1;
const RECORDS = 2;
const REFUSED = 8;
const CLOSED = 10;

const encoder = new TextEncoder();
const strictly = new TextDecoder("utf-8", { fatal: true });

function joined(...parts) {
  const whole = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let at = 0;
  for (const part of parts) {
    whole.set(part, at);
    at += part.length;
  }
  return whole;
}

function hex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function sha256(bytes) {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
}

async function hkdf(secret, salt, info, length) {
  const key = await crypto.subtle.importKey("raw", secret, "HKDF", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits(
    { name: "HKDF", hash: "SHA-256", salt: encoder.encode(salt), info: encoder.encode(info) },
    key,
    length * 8,
  );
  return new Uint8Array(bits);
}

function symbolValue(letter) {
  let shouted = letter.toUpperCase();
  if (shouted === "I" || shouted === "L") shouted = "1";
  if (shouted === "O") shouted = "0";
  return ALPHABET.indexOf(shouted);
}

async function readSpelled(code, length) {
  const symbols = Array.from(code).filter((letter) => letter !== "-" && letter !== " " && letter !== "\t");
  const spelling = Math.ceil((length * 8) / 5);
  if (symbols.length !== spelling + 3) return null;
  const values = symbols.map(symbolValue);
  if (values.some((value) => value < 0)) return null;

  const bytes = new Uint8Array(length);
  let held = 0;
  let bits = 0;
  let at = 0;
  for (const value of values.slice(0, spelling)) {
    held = ((held << 5) | value) & 0xffff;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      if (at >= length) return null;
      bytes[at] = (held >> bits) & 0xff;
      at += 1;
    }
  }
  if (at !== length || (held & ((1 << bits) - 1)) !== 0) return null;

  const digest = await sha256(bytes);
  for (let check = 0; check < 3; check += 1) {
    if (values[spelling + check] !== (digest[check] & 0x1f)) return null;
  }
  return bytes;
}

function committedSpace(commitment) {
  return joined(encoder.encode(COMMITTED_MARK), commitment);
}

async function keysOf(code) {
  const mark = code.slice(0, 2).toUpperCase();
  const spelled = code.slice(2);
  if (mark !== "E-" && mark !== "V-") return null;

  const short = await readSpelled(spelled, 20);
  let secret;
  let salt;
  let space = null;
  if (short !== null && mark === "E-") {
    secret = await hkdf(short, SALT_FIRST_TO_CLAIM, "view", 20);
    salt = SALT_FIRST_TO_CLAIM;
  } else if (short !== null) {
    secret = short;
    salt = SALT_FIRST_TO_CLAIM;
  } else {
    const long = await readSpelled(spelled, 32);
    if (long === null) return null;
    salt = SALT_COMMITTED;
    if (mark === "E-") {
      secret = await hkdf(long, SALT_COMMITTED, "view", 20);
      const written = await sha256(await hkdf(long, SALT_COMMITTED, "write", 32));
      const commitment = await sha256(joined(encoder.encode(COMMITMENT_SALT), written));
      space = committedSpace(commitment.slice(0, 12));
    } else {
      secret = long.slice(0, 20);
      space = committedSpace(long.slice(20));
    }
  }
  if (space === null) space = await hkdf(secret, salt, "space", 16);

  return {
    space,
    token: await hkdf(secret, salt, "read", 32),
    seal: await hkdf(secret, salt, "seal", 32),
  };
}

function rotated(value, by) {
  return (value << by) | (value >>> (32 - by));
}

function chachaBlock(key, counter, nonce) {
  const state = new Uint32Array(16);
  state[0] = 0x61707865;
  state[1] = 0x3320646e;
  state[2] = 0x79622d32;
  state[3] = 0x6b206574;
  const keyWords = new DataView(key.buffer, key.byteOffset, 32);
  for (let word = 0; word < 8; word += 1) state[4 + word] = keyWords.getUint32(word * 4, true);
  state[12] = counter;
  const nonceWords = new DataView(nonce.buffer, nonce.byteOffset, NONCE_BYTES);
  for (let word = 0; word < 3; word += 1) state[13 + word] = nonceWords.getUint32(word * 4, true);

  const mixed = Uint32Array.from(state);
  const quarter = (a, b, c, d) => {
    mixed[a] += mixed[b];
    mixed[d] = rotated(mixed[d] ^ mixed[a], 16);
    mixed[c] += mixed[d];
    mixed[b] = rotated(mixed[b] ^ mixed[c], 12);
    mixed[a] += mixed[b];
    mixed[d] = rotated(mixed[d] ^ mixed[a], 8);
    mixed[c] += mixed[d];
    mixed[b] = rotated(mixed[b] ^ mixed[c], 7);
  };
  for (let round = 0; round < 10; round += 1) {
    quarter(0, 4, 8, 12);
    quarter(1, 5, 9, 13);
    quarter(2, 6, 10, 14);
    quarter(3, 7, 11, 15);
    quarter(0, 5, 10, 15);
    quarter(1, 6, 11, 12);
    quarter(2, 7, 8, 13);
    quarter(3, 4, 9, 14);
  }

  const block = new Uint8Array(64);
  const blockWords = new DataView(block.buffer);
  for (let word = 0; word < 16; word += 1) {
    blockWords.setUint32(word * 4, (mixed[word] + state[word]) >>> 0, true);
  }
  return block;
}

function littleEndian(bytes) {
  let value = 0n;
  for (let at = bytes.length - 1; at >= 0; at -= 1) value = (value << 8n) | BigInt(bytes[at]);
  return value;
}

function poly1305(key, message) {
  const r = littleEndian(key.subarray(0, 16)) & 0x0ffffffc0ffffffc0ffffffc0fffffffn;
  const s = littleEndian(key.subarray(16, 32));
  const prime = (1n << 130n) - 5n;
  let accumulated = 0n;
  for (let at = 0; at < message.length; at += 16) {
    const block = message.subarray(at, at + 16);
    const padded = littleEndian(block) + (1n << BigInt(8 * block.length));
    accumulated = ((accumulated + padded) * r) % prime;
  }
  accumulated = (accumulated + s) & ((1n << 128n) - 1n);

  const tag = new Uint8Array(TAG_BYTES);
  for (let at = 0; at < TAG_BYTES; at += 1) {
    tag[at] = Number(accumulated & 0xffn);
    accumulated >>= 8n;
  }
  return tag;
}

function lengthsOf(aad, sealed) {
  const lengths = new Uint8Array(16);
  const view = new DataView(lengths.buffer);
  view.setBigUint64(0, BigInt(aad.length), true);
  view.setBigUint64(8, BigInt(sealed.length), true);
  return lengths;
}

function paddedTo16(bytes) {
  return new Uint8Array((16 - (bytes.length % 16)) % 16);
}

function unsealed(key, aad, blob) {
  if (blob.length < NONCE_BYTES + TAG_BYTES) return null;
  const nonce = blob.slice(0, NONCE_BYTES);
  const sealed = blob.subarray(NONCE_BYTES, blob.length - TAG_BYTES);
  const tag = blob.subarray(blob.length - TAG_BYTES);

  const macKey = chachaBlock(key, 0, nonce).subarray(0, 32);
  const authenticated = joined(aad, paddedTo16(aad), sealed, paddedTo16(sealed), lengthsOf(aad, sealed));
  const expected = poly1305(macKey, authenticated);
  let differs = 0;
  for (let at = 0; at < TAG_BYTES; at += 1) differs |= expected[at] ^ tag[at];
  if (differs !== 0) return null;

  const plain = new Uint8Array(sealed.length);
  for (let at = 0; at < sealed.length; at += 64) {
    const stream = chachaBlock(key, 1 + at / 64, nonce);
    const end = Math.min(64, sealed.length - at);
    for (let within = 0; within < end; within += 1) plain[at + within] = sealed[at + within] ^ stream[within];
  }
  return plain;
}

async function inflated(packed, ceiling) {
  try {
    const reader = new Blob([packed]).stream().pipeThrough(new DecompressionStream("deflate-raw")).getReader();
    const parts = [];
    let total = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.length;
      if (total > ceiling) {
        await reader.cancel();
        return null;
      }
      parts.push(value);
    }
    return joined(...parts);
  } catch {
    return null;
  }
}

class Reader {
  constructor(bytes) {
    this.bytes = bytes;
    this.at = 0;
  }

  byte() {
    if (this.at >= this.bytes.length) throw new RangeError();
    const value = this.bytes[this.at];
    this.at += 1;
    return value;
  }

  number() {
    let value = 0n;
    for (let shift = 0n; shift < 64n; shift += 7n) {
      const byte = this.byte();
      value |= BigInt(byte & 0x7f) << shift;
      if ((byte & 0x80) === 0) return value;
    }
    throw new RangeError();
  }

  small() {
    const value = this.number();
    if (value > BigInt(Number.MAX_SAFE_INTEGER)) throw new RangeError();
    return Number(value);
  }

  signed() {
    const value = this.number();
    return BigInt.asIntN(64, (value >> 1n) ^ -(value & 1n));
  }

  fixed(length) {
    if (this.at + length > this.bytes.length) throw new RangeError();
    const taken = this.bytes.subarray(this.at, this.at + length);
    this.at += length;
    return taken;
  }

  blob() {
    return this.fixed(this.small());
  }

  text() {
    return strictly.decode(this.blob());
  }

  flag() {
    const value = this.byte();
    if (value > 1) throw new RangeError();
    return value === 1;
  }

  done() {
    if (this.at !== this.bytes.length) throw new RangeError();
  }
}

function stampOf(reader) {
  return { nanos: reader.signed(), device: hex(reader.fixed(16)) };
}

function later(one, other) {
  return one.nanos > other.nanos || (one.nanos === other.nanos && one.device > other.device);
}

function recordingOf(reader) {
  if (reader.flag()) reader.text();
  const title = reader.text();
  const artist = reader.text();
  const album = reader.text();
  const ms = reader.flag() ? reader.small() : null;
  return { title, artist, album, ms };
}

function placeOf(reader) {
  const place = reader.blob();
  if (place.length === 0 || place[place.length - 1] === 0) throw new RangeError();
  return hex(place);
}

function opOf(bytes) {
  const reader = new Reader(bytes);
  const kind = reader.byte();
  let op;
  if (kind === ADDED) {
    const entry = hex(reader.fixed(16));
    const recording = recordingOf(reader);
    const place = placeOf(reader);
    if (reader.flag()) reader.fixed(16);
    op = { kind, entry, recording, place, stamp: stampOf(reader) };
  } else if (kind === MOVED) {
    const entry = hex(reader.fixed(16));
    const place = placeOf(reader);
    op = { kind, entry, place, stamp: stampOf(reader) };
  } else if (kind === REMOVED) {
    op = { kind, entry: hex(reader.fixed(16)), stamp: stampOf(reader) };
  } else if (kind === RENAMED) {
    const name = reader.text();
    op = { kind, name, stamp: stampOf(reader) };
  } else {
    return null;
  }
  reader.done();
  return op;
}

function opsOf(plain) {
  const reader = new Reader(plain);
  const count = reader.small();
  const ops = [];
  for (let at = 0; at < count; at += 1) {
    const op = opOf(reader.blob());
    if (op !== null) ops.push(op);
  }
  reader.done();
  return ops;
}

class List {
  constructor() {
    this.name = null;
    this.rows = new Map();
  }

  rowOf(entry) {
    if (!this.rows.has(entry) && this.rows.size >= ENTRIES_KEPT_AT_MOST) {
      let largest = null;
      for (const held of this.rows.keys()) if (largest === null || held > largest) largest = held;
      if (entry > largest) return null;
      this.rows.delete(largest);
    }
    if (!this.rows.has(entry)) this.rows.set(entry, { recording: null, placed: null, removed: null });
    return this.rows.get(entry);
  }

  placeLater(row, place, stamp) {
    if (row.placed === null || later(stamp, row.placed.stamp)) row.placed = { place, stamp };
  }

  apply(op) {
    if (op.kind === RENAMED) {
      if (this.name === null || later(op.stamp, this.name.stamp)) this.name = { name: op.name, stamp: op.stamp };
      return;
    }
    const row = this.rowOf(op.entry);
    if (row === null) return;
    if (op.kind === REMOVED) {
      row.recording = null;
      row.placed = null;
      row.removed = row.removed === null || later(op.stamp, row.removed) ? op.stamp : row.removed;
      return;
    }
    if (row.removed !== null) return;
    if (op.kind === ADDED && row.recording === null) row.recording = op.recording;
    this.placeLater(row, op.place, op.stamp);
  }

  shown() {
    const rows = [];
    for (const [entry, row] of this.rows) {
      if (row.recording !== null && row.placed !== null) rows.push({ entry, place: row.placed.place, recording: row.recording });
    }
    rows.sort((one, other) => {
      if (one.place !== other.place) return one.place < other.place ? -1 : 1;
      return one.entry < other.entry ? -1 : one.entry > other.entry ? 1 : 0;
    });
    return {
      name: this.name === null ? null : this.name.name,
      rows: rows.slice(0, ROWS_AT_MOST).map((row) => row.recording),
    };
  }
}

function viewAsked(keys) {
  const parts = [VIEW];
  const number = (value) => {
    let left = BigInt(value);
    for (;;) {
      const low = Number(left & 0x7fn);
      left >>= 7n;
      if (left === 0n) {
        parts.push(low);
        return;
      }
      parts.push(low | 0x80);
    }
  };
  number(PROTOCOL_VERSION);
  parts.push(...keys.space, ...keys.token, ...crypto.getRandomValues(new Uint8Array(16)));
  number(0);
  return new Uint8Array(parts);
}

function skipAddress(reader) {
  const family = reader.byte();
  reader.fixed(family === 4 ? 4 : family === 6 ? 16 : Infinity);
  reader.number();
}

async function readShared(code, address) {
  if (typeof crypto === "undefined" || crypto.subtle === undefined) return { failed: "unread" };
  const keys = await keysOf(code);
  if (keys === null) return { failed: "unread" };

  const list = new List();
  const take = async (blob) => {
    const packed = unsealed(keys.seal, keys.space, blob);
    if (packed === null) return;
    const plain = await inflated(packed, LARGEST_OPENED);
    if (plain === null) return;
    try {
      for (const op of opsOf(plain)) list.apply(op);
    } catch {
      return;
    }
  };

  return new Promise((answer) => {
    let socket;
    try {
      socket = new WebSocket(address);
    } catch {
      answer({ failed: "unreached" });
      return;
    }
    socket.binaryType = "arraybuffer";
    let welcomed = false;
    let settled = false;
    let handling = Promise.resolve();
    const settle = (said) => {
      if (settled) return;
      settled = true;
      clearTimeout(waiting);
      try {
        socket.close();
      } catch {
        socket = null;
      }
      answer(said);
    };
    const waiting = setTimeout(() => settle({ failed: "unreached" }), ANSWERED_WITHIN_MS);
    const read = async (bytes) => {
      const reader = new Reader(bytes);
      const kind = reader.byte();
      if (kind === WELCOME) {
        const online = reader.small();
        reader.fixed(online * 16);
        skipAddress(reader);
        reader.number();
        welcomed = true;
      } else if (kind === SNAPSHOT) {
        reader.number();
        const parts = reader.small();
        for (let at = 0; at < parts; at += 1) await take(reader.blob());
      } else if (kind === RECORDS) {
        const entries = reader.small();
        for (let at = 0; at < entries; at += 1) {
          reader.number();
          if (reader.flag()) await take(reader.blob());
        }
      } else if (kind === REFUSED) {
        settle({ failed: "unknown" });
        return;
      } else if (kind === CLOSED) {
        settle({ failed: "moved" });
        return;
      }
    };
    socket.onopen = () => socket.send(viewAsked(keys));
    socket.onmessage = (message) => {
      if (!(message.data instanceof ArrayBuffer)) return;
      const bytes = new Uint8Array(message.data);
      handling = handling.then(() => read(bytes)).catch(() => settle({ failed: "unread" }));
    };
    socket.onerror = () => {
      handling = handling.then(() => settle(welcomed ? list.shown() : { failed: "unreached" }));
    };
    socket.onclose = () => {
      handling = handling.then(() => settle(welcomed ? list.shown() : { failed: "unreached" }));
    };
  });
}

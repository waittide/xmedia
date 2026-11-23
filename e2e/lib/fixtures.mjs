import assert from 'node:assert/strict';
import { createCipheriv, createDecipheriv } from 'node:crypto';
import { deflateSync } from 'node:zlib';

// ---------------------------------------------------------------------------
// Deterministic valid binary media fixtures (generated in-memory / constant)
// ---------------------------------------------------------------------------

// Valid 16x16 JFIF JPEG (225 bytes, decodable by Chromium <img> and ffmpeg)
export const MOCK_JPEG_BYTES = Buffer.from(
  '/9j/4AAQSkZJRgABAgAAAQABAAD//gAQTGF2YzYzLjI0LjEwMAD/2wBDAAgEBAQEBAUFBQUFBQYGBgYGBgYGBgYGBgYHBwcICAgHBwcGBgcHCAgICAkJCQgICAgJCQoKCgwMCwsODg4RERT/xABMAAEBAAAAAAAAAAAAAAAAAAAABwEBAQAAAAAAAAAAAAAAAAAABQcQAQAAAAAAAAAAAAAAAAAAAAARAQAAAAAAAAAAAAAAAAAAAAD/wAARCAAQABADASIAAhEAAxEA/9oADAMBAAIRAxEAPwCOAL+Kf//Z',
  'base64',
);

// Valid 2x2 RGBA PNG with genuine CRC32 checksums (built via node:zlib)
const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) {
      c = (c & 1) !== 0 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[n] = c >>> 0;
  }
  return table;
})();

export function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function makePngChunk(type, data) {
  const typeBuf = Buffer.from(type, 'ascii');
  const lenBuf = Buffer.allocUnsafe(4);
  lenBuf.writeUInt32BE(data.length, 0);
  const crcInput = Buffer.concat([typeBuf, data]);
  const crcBuf = Buffer.allocUnsafe(4);
  crcBuf.writeUInt32BE(crc32(crcInput), 0);
  return Buffer.concat([lenBuf, typeBuf, data, crcBuf]);
}

function buildMockPng() {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.allocUnsafe(13);
  ihdr.writeUInt32BE(2, 0); // width = 2
  ihdr.writeUInt32BE(2, 4); // height = 2
  ihdr[8] = 8; // bit depth = 8
  ihdr[9] = 6; // color type = RGBA
  ihdr[10] = 0; // compression = deflate
  ihdr[11] = 0; // filter = adaptive
  ihdr[12] = 0; // interlace = none

  // 2 rows, each: filter byte (0) + 2 RGBA pixels (8 bytes) = 9 bytes/row
  const rawScanlines = Buffer.from([
    0x00, 0x22, 0x66, 0xcc, 0xff, 0x33, 0x88, 0xdd, 0xff,
    0x00, 0x44, 0x99, 0xee, 0xff, 0x55, 0xaa, 0xff, 0xff,
  ]);
  const compressed = deflateSync(rawScanlines);

  return Buffer.concat([
    signature,
    makePngChunk('IHDR', ihdr),
    makePngChunk('IDAT', compressed),
    makePngChunk('IEND', Buffer.alloc(0)),
  ]);
}

export const MOCK_PNG_BYTES = buildMockPng();

// Valid ID3v2.4 + 128 MPEG-1 Layer 3 silent frames (417 bytes/frame @ 128kbps 44.1kHz -> 53,386 bytes)
function buildMockMp3() {
  const id3 = Buffer.from([0x49, 0x44, 0x33, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
  const frame = Buffer.alloc(417, 0);
  frame[0] = 0xff;
  frame[1] = 0xfb;
  frame[2] = 0x90;
  frame[3] = 0x00;
  const parts = [id3];
  for (let i = 0; i < 128; i += 1) {
    parts.push(frame);
  }
  return Buffer.concat(parts);
}

export const MOCK_MP3_BYTES = buildMockMp3();

// Valid 188-byte aligned MPEG-TS segments (15 packets = 2820 bytes each, PAT+PMT+H.264/AAC PES)
export const MOCK_TS_SEGMENT_1 = Buffer.from(
  'R0AREABC8CUAAcEAAP8B/wAB/IAUSBIBBkZGbXBlZwlTZXJ2aWNlMDF3fEPK//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////9HQAAQAACwDQABwQAAAAHwACqxBLL//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////0dQABAAArAXAAHBAADhAPAAG+EA8AAP4QHwAC9EuZv/////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////R0EAMAdQAAB/IX4AAAAB4AAAgIAFIQAH6LUAAAABCfAAAAABZ0LACtkEJsBEAAADAAQAAAMAyDxImSAAAAABaMuDyyAAAAEGBf//a9xF6b3m2Ui3lizYINkj7u94MjY0IC0gY29yZSAxNjUgcjMyMjIgYjM1NjA1YSAtIEguMjY0L01QRUctNCBBVkMgY29kZWMgLSBDb3B5bGVmdCAyMDAzLTIwMjUgLSBodHRwOi8vd3d3LnZpZGVvbGFHAQARbi5vcmcveDI2NC5odG1sIC0gb3B0aW9uczogY2FiYWM9MCByZWY9MyBkZWJsb2NrPTE6MDowIGFuYWx5c2U9MHgxOjB4MTExIG1lPWhleCBzdWJtZT03IHBzeT0xIHBzeV9yZD0xLjAwOjAuMDAgbWl4ZWRfcmVmPTEgbWVfcmFuZ2U9MTYgY2hyb21hX21lPTEgdHJlbGxpcz0xIDh4OGRjdD0wIGNxbT0wIGRlYWR6b25lPTIxLEcBABIxMSBmYXN0X3Bza2lwPTEgY2hyb21hX3FwX29mZnNldD0tMiB0aHJlYWRzPTIgbG9va2FoZWFkX3RocmVhZHM9MSBzbGljZWRfdGhyZWFkcz0wIG5yPTAgZGVjaW1hdGU9MSBpbnRlcmxhY2VkPTAgYmx1cmF5X2NvbXBhdD0wIGNvbnN0cmFpbmVkX2ludHJhPTAgYmZyYW1lcz0wIHdlaWdodHA9MCBrZXlpbnQ9MTUga2V5aW50RwEAMwsA/////////////19taW49MSBzY2VuZWN1dD00MCBpbnRyYV9yZWZyZXNoPTAgcmNfbG9va2FoZWFkPTE1IHJjPWNyZiBtYnRyZWU9MSBjcmY9MjMuMCBxY29tcD0wLjYwIHFwbWluPTAgcXBtYXg9NjkgcXBzdGVwPTQgaXBfcmF0aW89MS40MCBhcT0xOjEuMDAAgAAAAWWIhAzxGKAAKWscAAT2o4AAhAycnJ1111111111115HQQA0mQD//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////wAAAeAAAICABSEACQTVAAAAAQnwAAAAAUGaOBnhGEdBADWZEAAAjTF+AP//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////AAAB4AAAgIAFIQAJIPUAAAABCfAAAAABQZpUBnhGR0AAEQAAsA0AAcEAAAAB8AAqsQSy//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////9HUAARAAKwFwABwQAA4QDwABvhAPAAD+EB8AAvRLmb/////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////0dBADaZAP//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////AAAB4AAAgIAFIQAJPRUAAAABCfAAAAABQZpgL8IwR0EAN5kQAACbQX4A//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////8AAAHgAACAgAUhAAlZNQAAAAEJ8AAAAAFBmoAvwjBHQQA4mQD//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////wAAAeAAAICABSEACXVVAAAAAQnwAAAAAUGaoCvCMEdBATAUQP////////////////////////8AAAHAAJ2AgAUhAAfYYf/xUEADn/zeAgBMYXZjNjMuMjQuMTAwAAAAQA7/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH',
  'base64',
);

export const MOCK_TS_SEGMENT_2 = Buffer.from(
  'R0AREABC8CUAAcEAAP8B/wAB/IAUSBIBBkZGbXBlZwlTZXJ2aWNlMDF3fEPK//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////9HQAAQAACwDQABwQAAAAHwACqxBLL//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////0dQABAAArAXAAHBAADhAPAAG+EA8AAP4QHwAC9EuZv/////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////R0EAMAdQAAB/IX4AAAAB4AAAgIAFIQAH6LUAAAABCfAAAAABZ0LACtkEJsBEAAADAAQAAAMAyDxImSAAAAABaMuDyyAAAAEGBf//a9xF6b3m2Ui3lizYINkj7u94MjY0IC0gY29yZSAxNjUgcjMyMjIgYjM1NjA1YSAtIEguMjY0L01QRUctNCBBVkMgY29kZWMgLSBDb3B5bGVmdCAyMDAzLTIwMjUgLSBodHRwOi8vd3d3LnZpZGVvbGFHAQARbi5vcmcveDI2NC5odG1sIC0gb3B0aW9uczogY2FiYWM9MCByZWY9MyBkZWJsb2NrPTE6MDowIGFuYWx5c2U9MHgxOjB4MTExIG1lPWhleCBzdWJtZT03IHBzeT0xIHBzeV9yZD0xLjAwOjAuMDAgbWl4ZWRfcmVmPTEgbWVfcmFuZ2U9MTYgY2hyb21hX21lPTEgdHJlbGxpcz0xIDh4OGRjdD0wIGNxbT0wIGRlYWR6b25lPTIxLEcBABIxMSBmYXN0X3Bza2lwPTEgY2hyb21hX3FwX29mZnNldD0tMiB0aHJlYWRzPTIgbG9va2FoZWFkX3RocmVhZHM9MSBzbGljZWRfdGhyZWFkcz0wIG5yPTAgZGVjaW1hdGU9MSBpbnRlcmxhY2VkPTAgYmx1cmF5X2NvbXBhdD0wIGNvbnN0cmFpbmVkX2ludHJhPTAgYmZyYW1lcz0wIHdlaWdodHA9MCBrZXlpbnQ9MTUga2V5aW50RwEAMwsA/////////////19taW49MSBzY2VuZWN1dD00MCBpbnRyYV9yZWZyZXNoPTAgcmNfbG9va2FoZWFkPTE1IHJjPWNyZiBtYnRyZWU9MSBjcmY9MjMuMCBxY29tcD0wLjYwIHFwbWluPTAgcXBtYXg9NjkgcXBzdGVwPTQgaXBfcmF0aW89MS40MCBhcT0xOjEuMDAAgAAAAWWIhAzxGKAAJPccAARN44AAntScnJ1111111111115HQQA0mQD//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////wAAAeAAAICABSEACQTVAAAAAQnwAAAAAUGaOBnhGEdBADWZEAAAjTF+AP//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////AAAB4AAAgIAFIQAJIPUAAAABCfAAAAABQZpUBnhGR0AAEQAAsA0AAcEAAAAB8AAqsQSy//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////9HUAARAAKwFwABwQAA4QDwABvhAPAAD+EB8AAvRLmb/////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////0dBADaZAP//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////AAAB4AAAgIAFIQAJPRUAAAABCfAAAAABQZpgL8IwR0EAN5kQAACbQX4A//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////8AAAHgAACAgAUhAAlZNQAAAAEJ8AAAAAFBmoAvwjBHQQA4mQD//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////wAAAeAAAICABSEACXVVAAAAAQnwAAAAAUGaoCvCMEdBATAUQP////////////////////////8AAAHAAJ2AgAUhAAfYYf/xUEADn/zeAgBMYXZjNjMuMjQuMTAwAAAAQA7/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH//FQQAF//AAAIAf/8VBAAX/8AAAgB//xUEABf/wAACAH',
  'base64',
);

// Valid playable H.264+AAC MP4 (2728 bytes: ftyp + moov + moof + mdat + mfra)
export const MOCK_MP4_BYTES = Buffer.from(
  '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',
  'base64',
);

// fMP4 init segment (ftyp + moov = 1251 bytes) & media fragment (moof + mdat = 1367 bytes)
export const MOCK_FMP4_INIT = MOCK_MP4_BYTES.subarray(0, 1251);
export const MOCK_FMP4_SEG = MOCK_MP4_BYTES.subarray(1251, 2618);

// Deterministic 16-byte AES-128 key (0x00..0x0f)
export const MOCK_AES_KEY_BYTES = Buffer.from([
  0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
]);

export function hlsSequenceIv(seq = 0) {
  const iv = Buffer.alloc(16, 0);
  iv.writeUInt32BE(seq >>> 0, 12);
  return iv;
}

export function encryptAes128Cbc(plainBuf, keyBytes = MOCK_AES_KEY_BYTES, seq = 0) {
  const cipher = createCipheriv('aes-128-cbc', keyBytes, hlsSequenceIv(seq));
  return Buffer.concat([cipher.update(toBuffer(plainBuf)), cipher.final()]);
}

export function decryptAes128Cbc(
  cipherBuf,
  keyBytes = MOCK_AES_KEY_BYTES,
  seq = 0,
  { partialPrefixBytes = null } = {},
) {
  const buf = toBuffer(cipherBuf);
  const decipher = createDecipheriv('aes-128-cbc', keyBytes, hlsSequenceIv(seq));
  if (partialPrefixBytes !== null) {
    decipher.setAutoPadding(false);
    const blockAligned = buf.subarray(0, Math.floor(buf.length / 16) * 16);
    const dec = Buffer.concat([decipher.update(blockAligned), decipher.final()]);
    return dec.subarray(0, partialPrefixBytes);
  }
  return Buffer.concat([decipher.update(buf), decipher.final()]);
}

// Valid GBK-encoded HTML containing "中文测试漫画" (GBK bytes: d6 d0 ce c4 b2 e2 ca d4 c2 fe bb ad)
export const MOCK_GBK_HTML_BYTES = Buffer.concat([
  Buffer.from('<html><head><meta charset="gbk"></head><body><h1 class="title">', 'ascii'),
  Buffer.from([0xd6, 0xd0, 0xce, 0xc4, 0xb2, 0xe2, 0xca, 0xd4, 0xc2, 0xfe, 0xbb, 0xad]),
  Buffer.from('</h1></body></html>', 'ascii'),
]);

// ---------------------------------------------------------------------------
// Magic-byte sniffers & assertion helpers
// ---------------------------------------------------------------------------

function toBuffer(input) {
  if (Buffer.isBuffer(input)) return input;
  if (input instanceof Uint8Array) return Buffer.from(input.buffer, input.byteOffset, input.byteLength);
  if (input instanceof ArrayBuffer) return Buffer.from(input);
  throw new TypeError(`Expected Buffer or Uint8Array, got ${typeof input}`);
}

function hasMarkupSignature(buf) {
  const headRaw = buf.subarray(0, Math.min(buf.length, 256)).toString('utf8');
  const head = headRaw.replace(/^\uFEFF/, '').trimStart().toLowerCase();
  if (
    head.startsWith('<svg') ||
    head.startsWith('<?xml') ||
    head.startsWith('<!doctype') ||
    head.startsWith('<html') ||
    head.startsWith('<head') ||
    head.startsWith('<body') ||
    head.includes('<svg') ||
    head.includes('<html') ||
    head.includes('<!doctype')
  ) {
    return head.slice(0, 64);
  }
  return null;
}

function rejectTextMarkup(buf, label) {
  const sig = hasMarkupSignature(buf);
  if (sig !== null) {
    throw new Error(`${label}: unexpected markup payload (${sig})`);
  }
}

export function sniffRasterImage(input) {
  const buf = toBuffer(input);
  if (buf.length < 4) return null;

  if (hasMarkupSignature(buf) !== null) {
    return null;
  }

  // JPEG: FF D8 FF
  if (buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) {
    return 'image/jpeg';
  }
  // PNG: 89 50 4E 47 0D 0A 1A 0A
  if (
    buf.length >= 24 &&
    buf[0] === 0x89 &&
    buf[1] === 0x50 &&
    buf[2] === 0x4e &&
    buf[3] === 0x47 &&
    buf[4] === 0x0d &&
    buf[5] === 0x0a &&
    buf[6] === 0x1a &&
    buf[7] === 0x0a
  ) {
    return 'image/png';
  }
  // WEBP: RIFF....WEBP
  if (
    buf.length >= 12 &&
    buf.subarray(0, 4).toString('ascii') === 'RIFF' &&
    buf.subarray(8, 12).toString('ascii') === 'WEBP'
  ) {
    return 'image/webp';
  }
  // GIF: GIF87a / GIF89a
  if (
    buf.length >= 6 &&
    (buf.subarray(0, 6).toString('ascii') === 'GIF87a' || buf.subarray(0, 6).toString('ascii') === 'GIF89a')
  ) {
    return 'image/gif';
  }
  // BMP: BM + 14-byte file header + >=12-byte DIB header (minimum 26 bytes)
  if (buf.length >= 26 && buf[0] === 0x42 && buf[1] === 0x4d) {
    return 'image/bmp';
  }
  return null;
}

export function assertRasterImage(input, expectedMime = null, label = 'raster image') {
  const buf = toBuffer(input);
  rejectTextMarkup(buf, label);
  const detected = sniffRasterImage(buf);
  assert.ok(detected, `${label}: unrecognized raster image magic bytes (first 16 bytes: ${buf.subarray(0, 16).toString('hex')})`);
  if (expectedMime) {
    assert.equal(detected, expectedMime, `${label}: expected ${expectedMime}, got ${detected}`);
  }
  return detected;
}

export function assertJpeg(input, label = 'JPEG') {
  const buf = toBuffer(input);
  assertRasterImage(buf, 'image/jpeg', label);
  assert.ok(buf.length >= 4, `${label}: buffer too small`);
  return 'image/jpeg';
}

export function assertPng(input, label = 'PNG') {
  const buf = toBuffer(input);
  assertRasterImage(buf, 'image/png', label);
  assert.equal(buf.subarray(12, 16).toString('ascii'), 'IHDR', `${label}: missing IHDR chunk`);
  return 'image/png';
}

export function assertMp3(input, label = 'MP3') {
  const buf = toBuffer(input);
  rejectTextMarkup(buf, label);
  assert.ok(buf.length >= 4, `${label}: MP3 buffer too short (${buf.length} bytes)`);
  const hasId3 = buf[0] === 0x49 && buf[1] === 0x44 && buf[2] === 0x33; // "ID3"
  const hasMpegSync = buf[0] === 0xff && (buf[1] & 0xe0) === 0xe0;
  assert.ok(
    hasId3 || hasMpegSync,
    `${label}: missing ID3 or MPEG sync header (got ${buf.subarray(0, 8).toString('hex')})`,
  );
  return 'audio/mpeg';
}

export function assertMpegTs(input, labelOrOpts = 'MPEG-TS', maybeOpts = {}) {
  let label = 'MPEG-TS';
  let opts = maybeOpts || {};
  if (typeof labelOrOpts === 'string') {
    label = labelOrOpts;
  } else if (labelOrOpts && typeof labelOrOpts === 'object') {
    opts = labelOrOpts;
    if (typeof opts.label === 'string') {
      label = opts.label;
    }
  }
  const buf = toBuffer(input);
  rejectTextMarkup(buf, label);
  assert.ok(buf.length >= 188, `${label}: MPEG-TS buffer too short (${buf.length} < 188 bytes)`);
  assert.equal(buf[0], 0x47, `${label}: missing 0x47 sync byte at offset 0`);
  if (opts.allowPartialTail !== true) {
    assert.equal(
      buf.length % 188,
      0,
      `${label}: MPEG-TS buffer length ${buf.length} is not a multiple of 188 bytes`,
    );
  }
  for (let off = 0; off < buf.length; off += 188) {
    assert.equal(buf[off], 0x47, `${label}: missing 0x47 sync byte at packet offset ${off}`);
  }
  return 'video/mp2t';
}

export function assertMp4(input, label = 'MP4', { allowFragment = false } = {}) {
  const buf = toBuffer(input);
  rejectTextMarkup(buf, label);
  assert.ok(buf.length >= 12, `${label}: MP4 buffer too short (${buf.length} < 12 bytes)`);
  const boxType = buf.subarray(4, 8).toString('ascii');
  if (allowFragment) {
    assert.ok(
      boxType === 'ftyp' || boxType === 'moof' || boxType === 'styp',
      `${label}: expected ftyp/moof/styp box at offset 4, got ${JSON.stringify(boxType)}`,
    );
  } else {
    assert.equal(boxType, 'ftyp', `${label}: expected ftyp box at offset 4, got ${JSON.stringify(boxType)}`);
  }
  return 'video/mp4';
}

function getHeaderCaseInsensitive(headers, name) {
  const target = name.toLowerCase();
  if (headers && typeof headers.get === 'function') {
    return headers.get(target);
  }
  if (headers && typeof headers === 'object') {
    for (const [k, v] of Object.entries(headers)) {
      if (k.toLowerCase() === target) {
        return Array.isArray(v) ? v.join(', ') : String(v);
      }
    }
  }
  return null;
}

export function assertSecurityHeaders(headers, { requireCsp = true, label = 'proxy response' } = {}) {
  const nosniff = getHeaderCaseInsensitive(headers, 'x-content-type-options');
  assert.ok(
    nosniff && nosniff.toLowerCase().includes('nosniff'),
    `${label}: expected X-Content-Type-Options: nosniff, got ${JSON.stringify(nosniff)}`,
  );
  if (requireCsp) {
    const csp = getHeaderCaseInsensitive(headers, 'content-security-policy');
    assert.ok(csp && csp.trim().length > 0, `${label}: missing Content-Security-Policy header`);
    const lowerCsp = csp.toLowerCase();
    assert.ok(
      lowerCsp.includes('default-src') || lowerCsp.includes('sandbox'),
      `${label}: non-restrictive CSP header: ${csp}`,
    );
    assert.ok(
      !lowerCsp.includes("'unsafe-inline'"),
      `${label}: unsafe-inline forbidden in CSP header: ${csp}`,
    );
    const directives = lowerCsp
      .split(';')
      .map((d) => d.trim())
      .filter(Boolean);
    for (const dir of directives) {
      const tokens = dir.split(/\s+/);
      const name = tokens[0];
      if (name === 'default-src' || name === 'img-src' || name === 'script-src') {
        assert.ok(
          !tokens.slice(1).includes('*'),
          `${label}: wildcard '*' forbidden in ${name} CSP directive: ${csp}`,
        );
      }
    }
  }
}

function assertProxyUri(uri, expectedProxyPath, expectedSourceId, contextLabel) {
  assert.ok(uri && uri.trim().length > 0, `${contextLabel}: empty URI`);
  let parsed;
  try {
    parsed = new URL(uri, 'http://127.0.0.1');
  } catch (err) {
    throw new Error(`${contextLabel}: invalid URI ${JSON.stringify(uri)}: ${err.message}`);
  }
  assert.equal(
    parsed.pathname,
    expectedProxyPath,
    `${contextLabel}: expected rewritten path ${expectedProxyPath}, got ${parsed.pathname} in ${uri}`,
  );
  const targetUrl = parsed.searchParams.get('url');
  assert.ok(targetUrl && targetUrl.startsWith('http'), `${contextLabel}: missing upstream url= param in ${uri}`);
  if (expectedSourceId !== undefined && expectedSourceId !== null) {
    assert.equal(
      parsed.searchParams.get('source_id'),
      String(expectedSourceId),
      `${contextLabel}: source_id mismatch in ${uri}`,
    );
  }
  return uri;
}

export function assertM3u8Rewritten(playlistText, optsOrSourceId = {}) {
  const opts =
    typeof optsOrSourceId === 'string'
      ? { expectedSourceId: optsOrSourceId }
      : optsOrSourceId || {};
  const {
    expectedProxyPath = '/api/v1/video/online/hls-proxy',
    expectedSourceId = null,
    requireKey = false,
    requireMap = false,
    minSegments = 1,
    label = 'rewritten m3u8',
  } = opts;
  assert.equal(typeof playlistText, 'string', `${label}: playlist must be string`);
  const lines = playlistText.replace(/\r\n/g, '\n').split('\n').map((l) => l.trim());
  const nonEmpty = lines.filter((l) => l.length > 0);
  assert.ok(nonEmpty.length > 0 && nonEmpty[0] === '#EXTM3U', `${label}: missing #EXTM3U header`);

  const segments = [];
  const keys = [];
  const maps = [];
  const tagUris = [];

  for (const line of nonEmpty) {
    if (line.startsWith('#EXT-X-')) {
      const uriMatches = [...line.matchAll(/URI=(?:"([^"]*)"|'([^']*)')/g)];
      if (uriMatches.length > 0) {
        for (const m of uriMatches) {
          const rawUri = m[1] ?? m[2] ?? '';
          const rewritten = assertProxyUri(
            rawUri,
            expectedProxyPath,
            expectedSourceId,
            `${label} ${line.split(':')[0]}`,
          );
          if (line.startsWith('#EXT-X-KEY:')) keys.push(rewritten);
          else if (line.startsWith('#EXT-X-MAP:')) maps.push(rewritten);
          else tagUris.push(rewritten);
        }
      } else if (
        (line.startsWith('#EXT-X-KEY:') && !line.includes('METHOD=NONE')) ||
        line.startsWith('#EXT-X-MAP:')
      ) {
        throw new Error(`${label}: tag missing URI="..." attribute: ${line}`);
      }
      continue;
    }
    if (line.startsWith('#')) continue;
    const rewritten = assertProxyUri(line, expectedProxyPath, expectedSourceId, `${label} URI line`);
    segments.push(rewritten);
  }

  if (requireKey) {
    assert.ok(keys.length >= 1, `${label}: expected at least 1 rewritten #EXT-X-KEY URI`);
  }
  if (requireMap) {
    assert.ok(maps.length >= 1, `${label}: expected at least 1 rewritten #EXT-X-MAP URI`);
  }
  assert.ok(
    segments.length >= minSegments,
    `${label}: expected >= ${minSegments} rewritten URI lines, got ${segments.length}`,
  );

  return { segments, keys, maps, tagUris };
}

export function parseByteRange(rangeHeader, totalLength) {
  if (!rangeHeader || typeof rangeHeader !== 'string') {
    return { present: false };
  }
  const m = rangeHeader.trim().match(/^bytes=(\d*)-(\d*)$/i);
  if (!m) {
    return { present: true, valid: false, status: 400 };
  }
  const [, rawStart, rawEnd] = m;
  if (rawStart === '' && rawEnd === '') {
    return { present: true, valid: false, status: 400 };
  }
  if (totalLength <= 0) {
    return { present: true, valid: false, status: 416 };
  }

  let start;
  let end;
  if (rawStart === '') {
    // Suffix byte range: bytes=-512
    const suffix = Number.parseInt(rawEnd, 10);
    if (!Number.isFinite(suffix) || suffix <= 0) {
      return { present: true, valid: false, status: 416 };
    }
    start = Math.max(0, totalLength - suffix);
    end = totalLength - 1;
  } else {
    start = Number.parseInt(rawStart, 10);
    if (!Number.isFinite(start) || start >= totalLength) {
      return { present: true, valid: false, status: 416 };
    }
    if (rawEnd === '') {
      end = totalLength - 1;
    } else {
      end = Number.parseInt(rawEnd, 10);
      if (!Number.isFinite(end) || end < start) {
        return { present: true, valid: false, status: 416 };
      }
      end = Math.min(end, totalLength - 1);
    }
  }
  return { present: true, valid: true, status: 206, start, end };
}

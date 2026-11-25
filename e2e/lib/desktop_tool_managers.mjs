import { execFile, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import zlib from 'node:zlib';
import { REPO_ROOT, allocateFreePort } from './client.mjs';

const execFileAsync = promisify(execFile);

const activeSubManagers = new Set();
let subSignalHandlersInstalled = false;

function cleanupAllSubManagersSync() {
  for (const mgr of [...activeSubManagers]) {
    mgr.forceCleanupSync();
  }
}

function attachSubSignalHandlers() {
  if (subSignalHandlersInstalled) return;
  subSignalHandlersInstalled = true;
  process.on('exit', cleanupAllSubManagersSync);
}

export async function ensureToolBinary() {
  const targetDir = process.env.CARGO_TARGET_DIR;
  const candidates = [
    process.env.XMEDIA_TOOL_BIN,
    targetDir ? path.join(targetDir, 'debug/xmedia-tool') : null,
    targetDir ? path.join(targetDir, 'release/xmedia-tool') : null,
    path.join(REPO_ROOT, 'target/debug/xmedia-tool'),
    path.join(REPO_ROOT, 'target/release/xmedia-tool'),
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return path.resolve(candidate);
    }
  }

  await execFileAsync(
    'cargo',
    ['build', '--manifest-path', path.join(REPO_ROOT, 'Cargo.toml'), '-p', 'xmedia-tool', '--bin', 'xmedia-tool'],
    { cwd: REPO_ROOT, timeout: 300000 },
  );

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return path.resolve(candidate);
    }
  }
  throw new Error(`xmedia-tool binary not found after build. Checked: ${candidates.join(', ')}`);
}

export async function ensureStandaloneBinary() {
  const targetDir = process.env.CARGO_TARGET_DIR;
  const candidates = [
    process.env.XMEDIA_APP_BIN,
    targetDir ? path.join(targetDir, 'debug/xmedia-app') : null,
    targetDir ? path.join(targetDir, 'release/xmedia-app') : null,
    path.join(REPO_ROOT, 'apps/xmedia-app/target/debug/xmedia-app'),
    path.join(REPO_ROOT, 'apps/xmedia-app/target/release/xmedia-app'),
    path.join(REPO_ROOT, 'target/debug/xmedia-app'),
    path.join(REPO_ROOT, 'target/release/xmedia-app'),
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return path.resolve(candidate);
    }
  }

  await execFileAsync(
    'cargo',
    [
      'build',
      '--manifest-path',
      path.join(REPO_ROOT, 'apps/xmedia-app/Cargo.toml'),
      '--bin',
      'xmedia-app',
    ],
    { cwd: REPO_ROOT, timeout: 300000 },
  );

  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return path.resolve(candidate);
    }
  }
  throw new Error(`xmedia-app binary not found after build. Checked: ${candidates.join(', ')}`);
}

/**
 * Sends a raw HTTP/1.1 request using `node:http` so custom security headers
 * (`Host`, `Origin`, `Sec-Fetch-Site`, `Cookie`) are delivered verbatim without
 * Undici/Fetch forbidden-header stripping.
 */
export function rawHttpRequest(baseUrl, method, reqPath, { headers = {}, body = undefined } = {}) {
  const parsedBase = new URL(baseUrl);
  let payloadBuf = null;
  const outHeaders = { ...headers };

  if (body !== undefined && body !== null) {
    if (Buffer.isBuffer(body)) {
      payloadBuf = body;
    } else if (typeof body === 'string') {
      payloadBuf = Buffer.from(body, 'utf8');
    } else {
      payloadBuf = Buffer.from(JSON.stringify(body), 'utf8');
      if (!Object.keys(outHeaders).some((k) => k.toLowerCase() === 'content-type')) {
        outHeaders['Content-Type'] = 'application/json';
      }
    }
    if (!Object.keys(outHeaders).some((k) => k.toLowerCase() === 'content-length')) {
      outHeaders['Content-Length'] = String(payloadBuf.length);
    }
  }

  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        hostname: parsedBase.hostname,
        port: Number(parsedBase.port),
        path: reqPath,
        method,
        headers: outHeaders,
      },
      (res) => {
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => {
          const buffer = Buffer.concat(chunks);
          const text = buffer.toString('utf8');
          let json = null;
          try {
            json = JSON.parse(text);
          } catch {
            json = null;
          }
          const rawSetCookies = Array.isArray(res.headers['set-cookie'])
            ? res.headers['set-cookie']
            : res.headers['set-cookie']
              ? [res.headers['set-cookie']]
              : [];
          resolve({
            status: res.statusCode || 0,
            headers: res.headers,
            rawSetCookies,
            buffer,
            text,
            json,
          });
        });
      },
    );
    req.on('error', reject);
    if (payloadBuf) {
      req.write(payloadBuf);
    }
    req.end();
  });
}

export function extractSetCookiePair(rawSetCookies, cookieName) {
  for (const raw of rawSetCookies) {
    const first = raw.split(';')[0]?.trim() || '';
    const eq = first.indexOf('=');
    if (eq > 0 && first.slice(0, eq).trim() === cookieName) {
      return {
        raw,
        value: first.slice(eq + 1).trim(),
      };
    }
  }
  return null;
}

/**
 * Minimal ZIP reader supporting STORE (0) and DEFLATE (8) entries via Central Directory.
 */
export function readZipEntries(zipBuf) {
  let eocdOffset = -1;
  const minEocd = Math.max(0, zipBuf.length - 65557);
  for (let i = zipBuf.length - 22; i >= minEocd; i--) {
    if (zipBuf.readUInt32LE(i) === 0x06054b50) {
      eocdOffset = i;
      break;
    }
  }
  if (eocdOffset < 0) {
    throw new Error('Invalid ZIP buffer: EOCD signature not found');
  }

  const totalEntries = zipBuf.readUInt16LE(eocdOffset + 10);
  let cdOffset = zipBuf.readUInt32LE(eocdOffset + 16);
  const entries = new Map();

  for (let idx = 0; idx < totalEntries; idx++) {
    if (zipBuf.readUInt32LE(cdOffset) !== 0x02014b50) {
      throw new Error(`Invalid Central Directory header at offset ${cdOffset}`);
    }
    const method = zipBuf.readUInt16LE(cdOffset + 10);
    const compressedSize = zipBuf.readUInt32LE(cdOffset + 20);
    const fileNameLen = zipBuf.readUInt16LE(cdOffset + 28);
    const extraLen = zipBuf.readUInt16LE(cdOffset + 30);
    const commentLen = zipBuf.readUInt16LE(cdOffset + 32);
    const localHeaderOffset = zipBuf.readUInt32LE(cdOffset + 42);
    const name = zipBuf.subarray(cdOffset + 46, cdOffset + 46 + fileNameLen).toString('utf8');

    if (zipBuf.readUInt32LE(localHeaderOffset) !== 0x04034b50) {
      throw new Error(`Invalid Local File header for ${name} at ${localHeaderOffset}`);
    }
    const localNameLen = zipBuf.readUInt16LE(localHeaderOffset + 26);
    const localExtraLen = zipBuf.readUInt16LE(localHeaderOffset + 28);
    const dataStart = localHeaderOffset + 30 + localNameLen + localExtraLen;
    const rawSlice = zipBuf.subarray(dataStart, dataStart + compressedSize);

    if (method === 0) {
      entries.set(name, Buffer.from(rawSlice));
    } else if (method === 8) {
      entries.set(name, zlib.inflateRawSync(rawSlice));
    } else {
      throw new Error(`Unsupported ZIP compression method ${method} for entry ${name}`);
    }

    cdOffset += 46 + fileNameLen + extraLen + commentLen;
  }

  return entries;
}

async function waitForHttpReady(url, proc, stderrChunks, timeoutMs = 20000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) {
      throw new Error(
        `Process exited prematurely with code ${proc.exitCode}: ${Buffer.concat(stderrChunks).toString('utf8')}`,
      );
    }
    try {
      const res = await rawHttpRequest(url, 'GET', new URL(url).pathname);
      if (res.status === 200) {
        return;
      }
    } catch {
      // retry until ready
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(
    `Timed out waiting for ${url} to become ready. Stderr: ${Buffer.concat(stderrChunks).toString('utf8')}`,
  );
}

export class ToolServerManager {
  constructor({ keepTmp = false } = {}) {
    this.keepTmp = keepTmp;
    this.tmpRoot = null;
    this.dataDir = null;
    this.mediaDir = null;
    this.port = 0;
    this.baseUrl = null;
    this.proc = null;
  }

  async start({ password = null, lockRoots = false, includeDefaultRoot = true } = {}) {
    const bin = await ensureToolBinary();
    this.port = await allocateFreePort('127.0.0.1');
    this.tmpRoot = mkdtempSync(path.join(os.tmpdir(), 'xmedia-e2e-tool-'));
    this.dataDir = path.join(this.tmpRoot, 'data');
    this.mediaDir = path.join(this.tmpRoot, 'media');
    mkdirSync(this.dataDir, { recursive: true });
    mkdirSync(this.mediaDir, { recursive: true });

    const args = ['--bind', `127.0.0.1:${this.port}`, '--data-dir', this.dataDir];
    if (includeDefaultRoot) {
      args.push('--root', this.mediaDir);
    }
    if (lockRoots) {
      args.push('--lock-roots');
    }
    if (password) {
      args.push('--password', password);
    }

    const stderrChunks = [];
    this.proc = spawn(bin, args, {
      cwd: REPO_ROOT,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    this.proc.stderr.on('data', (chunk) => stderrChunks.push(chunk));

    attachSubSignalHandlers();
    activeSubManagers.add(this);

    this.baseUrl = `http://127.0.0.1:${this.port}`;
    await waitForHttpReady(`${this.baseUrl}/api/v1/tool/status`, this.proc, stderrChunks);
    return this.baseUrl;
  }

  forceCleanupSync() {
    activeSubManagers.delete(this);
    if (this.proc && this.proc.exitCode === null) {
      try {
        this.proc.kill('SIGKILL');
      } catch {
        // ignore
      }
    }
    this.proc = null;
    if (this.tmpRoot && !this.keepTmp) {
      try {
        rmSync(this.tmpRoot, { recursive: true, force: true });
      } catch {
        // ignore
      }
    }
  }

  async cleanup() {
    activeSubManagers.delete(this);
    if (this.proc && this.proc.exitCode === null) {
      const p = this.proc;
      this.proc = null;
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          try {
            p.kill('SIGKILL');
          } catch {
            // ignore
          }
          resolve();
        }, 3000);
        p.once('exit', () => {
          clearTimeout(timer);
          resolve();
        });
        try {
          p.kill('SIGTERM');
        } catch {
          clearTimeout(timer);
          resolve();
        }
      });
    }
    if (this.tmpRoot && !this.keepTmp) {
      rmSync(this.tmpRoot, { recursive: true, force: true });
    }
  }
}

export class StandaloneGatewayManager {
  constructor({ keepTmp = false } = {}) {
    this.keepTmp = keepTmp;
    this.tmpRoot = null;
    this.dataDir = null;
    this.port = 0;
    this.baseUrl = null;
    this.proc = null;
  }

  async start() {
    const bin = await ensureStandaloneBinary();
    this.port = await allocateFreePort('127.0.0.1');
    this.tmpRoot = mkdtempSync(path.join(os.tmpdir(), 'xmedia-e2e-standalone-'));
    this.dataDir = path.join(this.tmpRoot, 'data');
    mkdirSync(this.dataDir, { recursive: true });

    const stderrChunks = [];
    this.proc = spawn(bin, ['--headless-gateway', `127.0.0.1:${this.port}`], {
      cwd: REPO_ROOT,
      env: {
        ...process.env,
        XMEDIA_APP_DATA_DIR: this.dataDir,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    this.proc.stderr.on('data', (chunk) => stderrChunks.push(chunk));

    attachSubSignalHandlers();
    activeSubManagers.add(this);

    this.baseUrl = `http://127.0.0.1:${this.port}`;
    await waitForHttpReady(`${this.baseUrl}/_app/health`, this.proc, stderrChunks);
    return this.baseUrl;
  }

  forceCleanupSync() {
    activeSubManagers.delete(this);
    if (this.proc && this.proc.exitCode === null) {
      try {
        this.proc.kill('SIGKILL');
      } catch {
        // ignore
      }
    }
    this.proc = null;
    if (this.tmpRoot && !this.keepTmp) {
      try {
        rmSync(this.tmpRoot, { recursive: true, force: true });
      } catch {
        // ignore
      }
    }
  }

  async cleanup() {
    activeSubManagers.delete(this);
    if (this.proc && this.proc.exitCode === null) {
      const p = this.proc;
      this.proc = null;
      await new Promise((resolve) => {
        const timer = setTimeout(() => {
          try {
            p.kill('SIGKILL');
          } catch {
            // ignore
          }
          resolve();
        }, 3000);
        p.once('exit', () => {
          clearTimeout(timer);
          resolve();
        });
        try {
          p.kill('SIGTERM');
        } catch {
          clearTimeout(timer);
          resolve();
        }
      });
    }
    if (this.tmpRoot && !this.keepTmp) {
      rmSync(this.tmpRoot, { recursive: true, force: true });
    }
  }
}

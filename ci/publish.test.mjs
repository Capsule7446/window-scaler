import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { loadBundle, publish } from './publish.mjs';

const sha = 'a'.repeat(40);
const options = { repo: 'owner/repo', version: '0.1.0', sha, assets: [{ name: 'WindowScaler.exe', data: Buffer.from('test') }] };
const draft = { id: 1, tag_name: 'v0.1.0', draft: true, upload_url: 'https://uploads.github.com/repos/owner/repo/releases/1/assets{?name,label}', assets: [], body: `<!-- window-scaler-release:${sha} -->` };

test('wrong tag commit is rejected before any write', async () => {
  const calls = [];
  await assert.rejects(publish({ ...options, api: async (method, path) => { calls.push(method); return { object: { type: 'commit', sha: 'b'.repeat(40) } }; } }), /refusing to overwrite/);
  assert.deepEqual(calls, ['GET']);
});
test('upload failure leaves the release in draft', async () => {
  const calls = [];
  const api = async (method, path, body, binary) => {
    calls.push(method);
    if (binary) throw new Error('upload failed');
    if (method === 'GET') return path.includes('?per_page=') ? [] : null;
    if (path.endsWith('/releases')) return draft;
    return {};
  };
  await assert.rejects(publish({ ...options, api }), /upload failed/);
  assert.ok(!calls.includes('PATCH'));
});
test('all assets upload before draft becomes public', async () => {
  const calls = [];
  const api = async (method, path, body, binary) => {
    calls.push({ method, body, binary });
    if (method === 'GET') return path.includes('?per_page=') ? [] : null;
    if (binary) return { size: body.length };
    if (method === 'POST' && path.endsWith('/releases')) return draft;
    return { html_url: 'https://github.com/owner/repo/releases/tag/v0.1.0' };
  };
  await publish({ ...options, assets: [...options.assets, { name: 'SHA256SUMS.txt', data: Buffer.from('checksum') }], api });
  assert.equal(calls.at(-1).method, 'PATCH');
  assert.equal(calls.at(-1).body.draft, false);
  assert.equal(calls.at(-1).body.make_latest, 'legacy');
  assert.equal(calls.filter(call => call.binary).length, 2);
  assert.ok(calls.at(-2).binary);
});
test('a failed draft is found on a later listing page and resumed without creating another release', async () => {
  const calls = [];
  const api = async (method, path, body, binary) => {
    calls.push({ method, path, body });
    if (path.includes('/git/ref/')) return { object: { type: 'commit', sha } };
    if (path.includes('/releases/tags/')) return null;
    if (path.endsWith('page=1')) return Array.from({ length: 100 }, (_, i) => ({ tag_name: `v9.0.${i}` }));
    if (path.endsWith('page=2')) return [{ ...draft, assets: [{ id: 7, name: 'WindowScaler.exe' }] }];
    if (binary) return { size: body.length };
    return { html_url: 'published' };
  };
  await publish({ ...options, api });
  assert.ok(!calls.some(call => call.method === 'POST' && call.path.endsWith('/releases')));
  assert.ok(calls.some(call => call.method === 'DELETE' && call.path.endsWith('/assets/7')));
  assert.equal(calls.at(-1).path, '/repos/owner/repo/releases/1');
});
test('a draft from another commit cannot be resumed or changed', async () => {
  const calls = [];
  const api = async (method, path) => {
    calls.push(method);
    if (path.includes('/git/ref/')) return { object: { type: 'commit', sha } };
    if (path.includes('?per_page=')) return [{ ...draft, body: 'another workflow' }];
    return null;
  };
  await assert.rejects(publish({ ...options, api }), /not owned/);
  assert.ok(calls.every(method => method === 'GET'));
});
test('published releases are immutable on rerun', async () => {
  const calls = [];
  const api = async (method, path) => {
    calls.push(method);
    return path.includes('/git/ref/') ? { object: { type: 'commit', sha } } : { ...draft, draft: false, assets: [{ name: 'WindowScaler.exe', size: 4 }], html_url: 'published' };
  };
  assert.equal((await publish({ ...options, api })).alreadyPublished, true);
  assert.deepEqual(calls, ['GET', 'GET']);
});
test('modified artifacts and mismatched commit metadata are rejected', () => {
  const directory = mkdtempSync(join(tmpdir(), 'window-scaler-bundle-'));
  try {
    writeFileSync(join(directory, 'WindowScaler.exe'), 'exe');
    writeFileSync(join(directory, 'WindowScaler-v0.1.0-windows-x86_64.zip'), 'zip');
    writeFileSync(join(directory, 'build-info.json'), JSON.stringify({ version: '0.1.0', commit: sha, target: 'x86_64-pc-windows-msvc' }));
    const names = ['WindowScaler.exe', 'WindowScaler-v0.1.0-windows-x86_64.zip', 'build-info.json'];
    writeFileSync(join(directory, 'SHA256SUMS.txt'), names.map(name => `${createHash('sha256').update(readFileSync(join(directory, name))).digest('hex')}  ${name}`).join('\n'));
    assert.equal(loadBundle(directory, '0.1.0', sha).length, 4);
    assert.throws(() => loadBundle(directory, '0.1.0', 'b'.repeat(40)), /metadata/);
    writeFileSync(join(directory, 'WindowScaler.exe'), 'changed');
    assert.throws(() => loadBundle(directory, '0.1.0', sha), /Checksum mismatch/);
  } finally {
    if (!directory.startsWith(join(tmpdir(), 'window-scaler-bundle-'))) throw new Error('Unexpected test directory');
    rmSync(directory, { recursive: true });
  }
});

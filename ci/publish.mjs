import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export function loadBundle(directory, version, sha) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) || !/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid release identity');
  const names = ['WindowScaler.exe', `WindowScaler-v${version}-windows-x86_64.zip`, 'build-info.json'];
  const info = JSON.parse(readFileSync(join(directory, 'build-info.json'), 'utf8').replace(/^\uFEFF/, ''));
  if (info.version !== version || info.commit !== sha || info.target !== 'x86_64-pc-windows-msvc') throw new Error('Artifact metadata does not match release commit');
  const manifest = readFileSync(join(directory, 'SHA256SUMS.txt'), 'utf8').trim().split(/\r?\n/);
  const expected = new Map();
  for (const line of manifest) {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    if (!match || !names.includes(match[2]) || expected.has(match[2])) throw new Error('Invalid checksum manifest');
    expected.set(match[2], match[1]);
  }
  if (expected.size !== names.length) throw new Error('Incomplete checksum manifest');
  const assets = names.map(name => {
    const data = readFileSync(join(directory, name));
    if (createHash('sha256').update(data).digest('hex') !== expected.get(name)) throw new Error(`Checksum mismatch: ${name}`);
    return { name, data };
  });
  return [...assets, { name: 'SHA256SUMS.txt', data: readFileSync(join(directory, 'SHA256SUMS.txt')) }];
}

export async function publish({ api, repo, version, sha, assets }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo) || !/^\d+\.\d+\.\d+$/.test(version) || !/^[a-f0-9]{40}$/.test(sha)) throw new Error('Invalid release parameters');
  const root = `/repos/${repo}`;
  const tag = `v${version}`;
  const ref = await api('GET', `${root}/git/ref/tags/${tag}`);
  if (ref && (ref.object.type !== 'commit' || ref.object.sha !== sha)) throw new Error('Existing tag points elsewhere; refusing to overwrite');
  let release = await api('GET', `${root}/releases/tags/${tag}`);
  if (!release) {
    // The by-tag endpoint only promises published releases. Authenticated listing
    // also exposes drafts, including partial uploads left by an earlier attempt.
    const matches = [];
    for (let page = 1; ; page++) {
      const entries = await api('GET', `${root}/releases?per_page=100&page=${page}`);
      if (!Array.isArray(entries)) throw new Error('Invalid release listing');
      matches.push(...entries.filter(entry => entry.tag_name === tag));
      if (entries.length < 100) break;
    }
    if (matches.length > 1) throw new Error('Multiple releases use this tag; manual investigation required');
    release = matches[0];
  }
  if (release && !release.draft) {
    if (!ref || assets.some(file => !release.assets.some(asset => asset.name === file.name && asset.size > 0))) throw new Error('Published release is incomplete; manual investigation required');
    return { url: release.html_url, alreadyPublished: true };
  }
  const marker = `<!-- window-scaler-release:${sha} -->`;
  if (release && !release.body?.includes(marker)) throw new Error('Existing draft is not owned by this workflow');
  if (!ref) await api('POST', `${root}/git/refs`, { ref: `refs/tags/${tag}`, sha });
  if (!release) {
    release = await api('POST', `${root}/releases`, {
      tag_name: tag, target_commitish: sha, name: `窗口缩放器 ${tag}`, draft: true, prerelease: false,
      body: `${marker}\nWindows x64正式包。下载ZIP解压运行WindowScaler.exe，或下载单独EXE。SHA256SUMS.txt用于校验文件。\n\n构建提交：${sha}\n\n[使用说明](https://github.com/${repo}/blob/${tag}/README.md) · [已知兼容范围](https://github.com/${repo}/blob/${tag}/docs/spec.md)`,
    });
  }
  const upload = new URL(release.upload_url.replace(/\{.*$/, ''));
  if (upload.protocol !== 'https:' || upload.hostname !== 'uploads.github.com') throw new Error('Unexpected upload destination');
  for (const file of assets) {
    const previous = release.assets?.find(asset => asset.name === file.name);
    if (previous) await api('DELETE', `${root}/releases/assets/${previous.id}`);
    const url = new URL(upload);
    url.searchParams.set('name', file.name);
    const uploaded = await api('POST', url.href, file.data, true);
    if (uploaded.size !== file.data.length) throw new Error(`Incomplete upload: ${file.name}`);
  }
  const published = await api('PATCH', `${root}/releases/${release.id}`, { draft: false, prerelease: false, make_latest: 'legacy' });
  return { url: published.html_url, alreadyPublished: false };
}

async function main() {
  if (process.env.GITHUB_EVENT_NAME !== 'push' || process.env.GITHUB_REF !== 'refs/heads/master') throw new Error('Official publication is only allowed from a master push');
  const { RELEASE_VERSION: version, GITHUB_SHA: sha, GITHUB_REPOSITORY: repo, GH_TOKEN: token } = process.env;
  if (!token) throw new Error('Missing GitHub token');
  const assets = loadBundle('artifacts', version, sha);
  const api = async (method, path, body, binary = false) => {
    const url = path.startsWith('https://') ? path : `https://api.github.com${path}`;
    const response = await fetch(url, { method,
      headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': binary ? 'application/octet-stream' : 'application/json' },
      body: body === undefined ? undefined : binary ? body : JSON.stringify(body),
    });
    if (response.status === 404 && method === 'GET') return null;
    if (!response.ok) throw new Error(`GitHub ${method} failed: HTTP ${response.status}`);
    return response.status === 204 ? null : response.json();
  };
  console.log(JSON.stringify(await publish({ api, repo, version, sha, assets })));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}

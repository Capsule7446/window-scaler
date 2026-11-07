import { readFileSync, appendFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const versionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
export function versionOf(branch) {
  const match = /^release\/v(.+)$/.exec(branch);
  if (!match || !versionPattern.test(match[1])) return null;
  return match[1];
}

export function packageVersion(manifest) {
  const section = manifest.split('[package]')[1]?.split(/\r?\n\[/)[0];
  const version = /^version\s*=\s*"([^"]+)"/m.exec(section ?? '')?.[1];
  if (!version || !versionPattern.test(version)) throw new Error('Cargo package version must be X.Y.Z');
  return version;
}

export function validateRoute(head, base) {
  if (base === 'master' && (versionOf(head) || head === 'release/ci-setup')) return;
  const epic = /^epic\/(\d+)-[a-z0-9][a-z0-9.-]*$/.exec(base);
  if (epic) {
    const feature = /^feature\/(\d+)\/\d+-[a-z0-9][a-z0-9.-]*$/.exec(head);
    if (feature?.[1] === epic[1] || /^hotfix\/\d+-[a-z0-9][a-z0-9.-]*$/.test(head) || head === 'master') return;
  }
  if ((versionOf(base) || base === 'release/ci-setup') && (/^epic\/\d+-[a-z0-9][a-z0-9.-]*$/.test(head) || /^hotfix\/\d+-[a-z0-9][a-z0-9.-]*$/.test(head))) return;
  throw new Error(`Invalid branch route: ${head} -> ${base}`);
}

export function releasePlan({ eventName, refName, version, sha, repo, pullRequest, associated = [] }) {
  const plan = { version, tag: `v${version}`, build: false, publish: false };
  if (!versionPattern.test(version)) throw new Error('Invalid package version');
  const verifyVersion = branch => {
    const requested = versionOf(branch);
    if (requested && requested !== version) throw new Error(`Release branch ${branch} does not match Cargo version ${version}`);
    return requested;
  };
  if (eventName === 'workflow_dispatch') return { ...plan, build: true };
  if (eventName === 'pull_request') {
    if (!pullRequest) throw new Error('Missing pull request');
    validateRoute(pullRequest.head.ref, pullRequest.base.ref);
    verifyVersion(pullRequest.head.ref);
    return { ...plan, build: pullRequest.base.ref === 'master' };
  }
  if (eventName !== 'push') throw new Error(`Unsupported event: ${eventName}`);
  if (refName.startsWith('release/')) {
    if (refName !== 'release/ci-setup' && !versionOf(refName)) throw new Error('Release branches must use release/vX.Y.Z');
    verifyVersion(refName);
    return { ...plan, build: true };
  }
  if (refName !== 'master') return plan;
  const candidates = associated.filter(pr => pr.merged_at && pr.base.ref === 'master'
    && pr.merge_commit_sha === sha && pr.head.repo?.full_name === repo && versionOf(pr.head.ref));
  if (candidates.length > 1) throw new Error('Ambiguous release PR for this master commit');
  if (candidates.length === 0) return plan;
  verifyVersion(candidates[0].head.ref);
  return { ...plan, build: true, publish: true };
}

async function main() {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const mode = process.argv[2] ?? 'plan';
  if (mode === 'route') {
    validateRoute(event.pull_request.head.ref, event.pull_request.base.ref);
    console.log(`Branch route accepted: ${event.pull_request.head.ref} -> ${event.pull_request.base.ref}`);
    return;
  }
  const version = packageVersion(readFileSync('Cargo.toml', 'utf8'));
  const repo = process.env.GITHUB_REPOSITORY;
  const sha = process.env.GITHUB_SHA;
  let associated = [];
  if (process.env.GITHUB_EVENT_NAME === 'push' && process.env.GITHUB_REF_NAME === 'master') {
    const response = await fetch(`https://api.github.com/repos/${repo}/commits/${sha}/pulls`, {
      headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${process.env.GH_TOKEN}`, 'X-GitHub-Api-Version': '2022-11-28' },
    });
    if (!response.ok) throw new Error(`Cannot query release PR: HTTP ${response.status}`);
    associated = await response.json();
  }
  const plan = releasePlan({ eventName: process.env.GITHUB_EVENT_NAME, refName: process.env.GITHUB_REF_NAME,
    repo, sha, version, pullRequest: event.pull_request, associated });
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT,
    Object.entries(plan).map(([key, value]) => `${key}=${value}`).join('\n') + '\n');
  console.log(JSON.stringify(plan));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}

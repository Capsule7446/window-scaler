import test from 'node:test';
import assert from 'node:assert/strict';
import { validateRoute, releasePlan, packageVersion } from './policy.mjs';

const base = { eventName: 'push', refName: 'master', version: '0.1.0', sha: 'a'.repeat(40), repo: 'owner/repo' };
const release = { merged_at: '2026-11-08', base: { ref: 'master' }, head: { ref: 'release/v0.1.0', repo: { full_name: 'owner/repo' } }, merge_commit_sha: base.sha };

test('feature can only merge into its own epic; master only receives release branches', () => {
  validateRoute('feature/5/6-release-ci', 'epic/5-window-scaler-v0.1');
  validateRoute('hotfix/9-size', 'release/v0.1.1');
  validateRoute('hotfix/9-ci-artifact-retry', 'release/ci-setup');
  validateRoute('release/v0.1.0', 'master');
  assert.throws(() => validateRoute('feature/5/6-release-ci', 'epic/8-other'));
  assert.throws(() => validateRoute('feature/5/6-release-ci', 'master'));
  assert.throws(() => validateRoute('hotfix/9-size', 'master'));
  assert.throws(() => validateRoute('feature/5/6-release-ci', 'release/ci-setup'));
});
test('release branch builds packages without publishing', () => {
  assert.deepEqual(releasePlan({ ...base, refName: 'release/v0.1.0' }), { version: '0.1.0', tag: 'v0.1.0', build: true, publish: false });
  assert.throws(() => releasePlan({ ...base, refName: 'release/v0.2.0' }));
  assert.throws(() => releasePlan({ ...base, refName: 'release/latest' }));
});
test('only the exact merged same-repository release PR publishes', () => {
  assert.equal(releasePlan({ ...base, associated: [release] }).publish, true);
  for (const altered of [{ ...release, merged_at: null }, { ...release, merge_commit_sha: 'b'.repeat(40) }, { ...release, head: { ...release.head, repo: { full_name: 'fork/repo' } } }]) {
    assert.equal(releasePlan({ ...base, associated: [altered] }).publish, false);
  }
  assert.equal(releasePlan(base).publish, false);
});
test('CI setup and manual validation never publish a product release', () => {
  assert.equal(releasePlan({ ...base, refName: 'release/ci-setup' }).publish, false);
  assert.equal(releasePlan({ ...base, associated: [{ ...release, head: { ...release.head, ref: 'release/ci-setup' } }] }).build, false);
  assert.equal(releasePlan({ ...base, eventName: 'workflow_dispatch' }).publish, false);
});
test('version is read from package section and must be a stable semantic version', () => {
  assert.equal(packageVersion('[package]\nversion = "0.1.0"\n[dependencies]\nversion = "9"'), '0.1.0');
  assert.throws(() => packageVersion('[package]\nversion = "0.1.0-beta.1"'));
});

import { mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// The provider fixture, the Web host and the specs have to agree on these paths.
// A spec asserting against a prompt log the endpoint never wrote is worse than no
// assertion, so the paths are resolved once here instead of being spelled out
// relative to each file that needs them. Everything lives under the ignored
// `.chaos/`, because a commit test that wrote into the tracked tree would be a
// test that dirties the repository it is testing.
const ignored = (relative: string) => fileURLToPath(new URL(`../../../../${relative}`, import.meta.url))

export const providerStateDir = process.env.CHAOS_E2E_PROVIDER_STATE_DIR || ignored('.chaos/e2e-provider')
export const gitWorkspace = process.env.CHAOS_E2E_GIT_WORKSPACE || ignored('.chaos/e2e-git-workspace')
export const promptLog = `${providerStateDir}/prompts.jsonl`
export const holdFile = `${providerStateDir}/hold`
/** A spec writes `{text, frameChars, gapMs}` here to make the endpoint answer that
 * one request slowly, in many small frames. It is a file rather than an environment
 * variable because the provider process is shared by every spec in the config: a
 * spec that needs a slow answer must not make the commit form wait for one. */
export const paceFile = `${providerStateDir}/pace.json`
/** One JSON line per answer the endpoint had to stop writing because the host hung
 * up mid-stream. It is the only place a cancelled turn can be observed on the side
 * that produced it, which is what makes a stop-button test about more than the UI. */
export const cutLog = `${providerStateDir}/cuts.jsonl`
export const providerReply = process.env.CHAOS_E2E_PROVIDER_REPLY || 'docs: 按暂存差异补充 note.txt 的说明'
export const providerPort = Number(process.env.CHAOS_E2E_PROVIDER_PORT || 8791)
export const providerModel = process.env.CHAOS_E2E_PROVIDER_MODEL || 'e2e-commit-model'
export const providerKey = process.env.CHAOS_E2E_PROVIDER_KEY || 'sk-e2e-commit-key'

/** Creates the directories the host and the fixture need before either starts. */
export function ensureE2eStateDirs() {
  mkdirSync(providerStateDir, { recursive: true })
  mkdirSync(gitWorkspace, { recursive: true })
}

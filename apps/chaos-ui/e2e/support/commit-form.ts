import { type Page } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { readdirSync, rmSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { gitWorkspace } from './paths'
import { headerTab } from './shell'

// Two specs drive the commit form: one against a normal host, one against Safe Web
// Mode. Both need the same repository and the same controls, so the preparation and
// the locators live here instead of being copied -- a copy is free to drift, and a
// drifted copy keeps passing. What each click has to prove stays in the spec.

/** Runs git inside the throwaway repository under the ignored `.chaos/`. */
export const git = (args: string[]) => execFileSync('git', ['-C', gitWorkspace, ...args], { encoding: 'utf8' }).trim()

/**
 * Rebuilds the repository one test needs: one commit named `base`, then a second
 * edit left in the index. A suggestion therefore has something real to read, and
 * `git log` can still show that no test committed anything of its own.
 */
export function resetRepository() {
  rmSync(resolve(gitWorkspace, '.git'), { recursive: true, force: true })
  for (const entry of readdirSync(gitWorkspace)) {
    rmSync(resolve(gitWorkspace, entry), { recursive: true, force: true })
  }
  git(['init', '-q'])
  git(['config', 'user.name', 'Chaos E2E'])
  git(['config', 'user.email', 'chaos-e2e@example.invalid'])
  writeFileSync(resolve(gitWorkspace, 'note.txt'), '第一版说明\n')
  git(['add', '--', 'note.txt'])
  git(['commit', '-q', '-m', 'base'])
  writeFileSync(resolve(gitWorkspace, 'note.txt'), '第二版说明\n')
  git(['add', '--', 'note.txt'])
}

export const gitTab = (page: Page) => headerTab(page, /Git/)
export const gitPanel = (page: Page) => page.locator('section[aria-label="Git 状态"]')
export const messageBox = (page: Page) => page.getByLabel('Git 提交信息')
export const suggestButton = (page: Page) => page.getByTestId('suggest-commit-message')

/**
 * The panel's own alert line. A second argument narrows it to one message: the Git
 * panel can hold two alerts at once (a refused status read and a refused
 * suggestion), and an assertion that matches either would pass on the wrong one.
 */
export const gitAlert = (page: Page, text?: string | RegExp) => {
  const alerts = gitPanel(page).getByRole('alert')
  return text === undefined ? alerts : alerts.filter({ hasText: text })
}

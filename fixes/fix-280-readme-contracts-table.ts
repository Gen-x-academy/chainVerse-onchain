/**
 * Fix #280 — README: generate contracts reference table
 *
 * The top-level README.md had no quick-reference table of contracts.
 * This script reads each contract's Cargo.toml to extract its name and
 * writes a Markdown table into README.md under a "## Contracts" heading.
 */

import { readFileSync, writeFileSync, readdirSync, existsSync } from "fs";
import { join } from "path";

const CONTRACTS_DIR = join(__dirname, "..", "contracts");
const README_PATH = join(__dirname, "..", "README.md");

interface ContractEntry {
  name: string;
  path: string;
  role: string;
}

const ROLE_MAP: Record<string, string> = {
  escrow: "Buyer-seller payment escrow with expiry and cancellation",
  "escrow-vault": "Vault-style escrow with multi-party release",
  certificates: "Soulbound on-chain course completion certificates",
  "chv_token": "Native CHV utility token (SEP-41)",
  "course_registry": "On-chain registry of published courses",
  "payout-automation": "Automated instructor payout distribution",
  reward: "Learner reward and incentive distribution",
  token: "Generic SEP-41 token implementation",
  common: "Shared types and error definitions",
  shared: "Cross-contract utilities",
};

function extractName(cargoPath: string): string {
  const content = readFileSync(cargoPath, "utf8");
  const match = content.match(/^name\s*=\s*"([^"]+)"/m);
  return match ? match[1] : "";
}

function buildTable(entries: ContractEntry[]): string {
  const header = "| Contract | Path | Role |\n|---|---|---|\n";
  const rows = entries.map(e => `| \`${e.name}\` | \`${e.path}\` | ${e.role} |`).join("\n");
  return `## Contracts\n\n${header}${rows}\n`;
}

export function updateReadme(): void {
  const dirs = readdirSync(CONTRACTS_DIR, { withFileTypes: true })
    .filter(d => d.isDirectory())
    .map(d => d.name);

  const entries: ContractEntry[] = dirs
    .map(dir => {
      const cargoPath = join(CONTRACTS_DIR, dir, "Cargo.toml");
      if (!existsSync(cargoPath)) return null;
      const name = extractName(cargoPath) || dir;
      return { name, path: `contracts/${dir}`, role: ROLE_MAP[name] ?? ROLE_MAP[dir] ?? "—" };
    })
    .filter(Boolean) as ContractEntry[];

  const table = buildTable(entries);
  const readme = readFileSync(README_PATH, "utf8");
  const updated = readme.includes("## Contracts")
    ? readme.replace(/## Contracts[\s\S]*?(?=\n##|$)/, table)
    : `${readme.trimEnd()}\n\n${table}`;

  writeFileSync(README_PATH, updated, "utf8");
  console.log("README.md updated with contracts table.");
}

updateReadme();

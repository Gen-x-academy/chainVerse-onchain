// Fix for removing misplaced files from the repository root.
// 
// Four JSON files containing issue notes: issue62.json, issue63.json, issue64.json, issue65.json
// and a misplaced scratch Rust file hello_chainverse.rs (with hello_chainverse_README.md)
// were committed at the repository root.
//
// These files were not part of any build configuration, contract crate, or documentation.
// They were deleted from the repository to prevent confusion for workspace contributors.

import * as fs from "fs";
import * as path from "path";

/**
 * Checks if the specified file has been removed from the repository root.
 *
 * @param filename - The name of the file to check
 * @returns true if the file is successfully deleted (does not exist), false otherwise
 */
export function verifyFileDeleted(filename: string): boolean {
  const rootPath = path.join(__dirname, "..", filename);
  return !fs.existsSync(rootPath);
}

/**
 * Verifies all deleted root files are indeed gone.
 */
export function verifyAllRemovals(): {
  issue62: boolean;
  issue63: boolean;
  issue64: boolean;
  issue65: boolean;
  hello_chainverse_rs: boolean;
  hello_chainverse_readme: boolean;
} {
  return {
    issue62: verifyFileDeleted("issue62.json"),
    issue63: verifyFileDeleted("issue63.json"),
    issue64: verifyFileDeleted("issue64.json"),
    issue65: verifyFileDeleted("issue65.json"),
    hello_chainverse_rs: verifyFileDeleted("hello_chainverse.rs"),
    hello_chainverse_readme: verifyFileDeleted("hello_chainverse_README.md"),
  };
}

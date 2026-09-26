#!/usr/bin/env python3
"""Tests for the scholarship interface gate.

The gate's only job is to fail on a breaking change and stay quiet on an
additive one. A gate that cannot be shown to catch a removed function is
indistinguishable from a gate that does nothing, so each breaking shape is
exercised against a fixture, and each is also shown to be catchable by
running the real diff function rather than trusting the classifier.

Fixtures are synthetic rather than copied from the contracts so the tests do
not break every time an unrelated function is added.
"""

import json
import os
import subprocess
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import scholarship_surface  # noqa: E402
from scholarship_surface import (  # noqa: E402
    diff_surfaces,
    find_consumers,
    load_override,
    parse_events,
    parse_functions,
)

BASE_SOURCE = """
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env};

#[contractimpl]
pub struct ProgramContract;

#[contractimpl]
impl ProgramContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        env.events()
            .publish((symbol_short!("PROGNEW"),), (admin,));
        Ok(())
    }

    pub fn get_program(env: Env, program_id: BytesN<32>) -> Result<Program, ContractError> {
        read_program(env, program_id)
    }

    pub fn reserve_award(
        env: Env,
        program_id: BytesN<32>,
        amount: i128,
    ) -> Result<(), ContractError> {
        write_award(env, program_id, amount)
    }

    pub fn version(_env: Env) -> u32 {
        1
    }
}
"""


def surface_of(source):
    return {
        "scholarship-test": {
            "functions": parse_functions(source),
            "events": parse_events(source),
        }
    }


def breaking_keys(old_source, new_source):
    surface = surface_of(new_source)
    return {
        c["key"]
        for c in diff_surfaces(surface_of(old_source), surface)
        if c["kind"] == "breaking"
    }


class TestFunctionParsing(unittest.TestCase):
    def setUp(self):
        self.functions = parse_functions(BASE_SOURCE)

    def test_finds_all_public_functions(self):
        self.assertEqual(
            sorted(self.functions),
            ["get_program", "initialize", "reserve_award", "version"],
        )

    def test_drops_env_parameter(self):
        # `env: Env` is not part of the on-chain interface, so renaming or
        # dropping it must not read as a breaking change.
        self.assertNotIn("env: Env", self.functions["initialize"]["params"])

    def test_captures_multiline_parameter_list(self):
        self.assertEqual(
            self.functions["reserve_award"]["params"],
            ["program_id: BytesN<32>", "amount: i128"],
        )

    def test_captures_return_types(self):
        self.assertEqual(self.functions["get_program"]["returns"], "Result<Program, ContractError>")
        self.assertEqual(self.functions["version"]["returns"], "u32")

    def test_ignores_test_helpers(self):
        source = BASE_SOURCE + "\npub fn test_internal_helper() {}\n"
        self.assertNotIn("test_internal_helper", parse_functions(source))


class TestEventParsing(unittest.TestCase):
    def test_finds_multiline_publish(self):
        # The publish call is split across lines; a line-oriented scan misses it.
        self.assertIn("PROGNEW", parse_events(BASE_SOURCE))

    def test_records_payload_arity(self):
        self.assertEqual(parse_events(BASE_SOURCE)["PROGNEW"], 1)

    def test_counts_multiple_payload_values(self):
        source = BASE_SOURCE.replace(".publish((symbol_short!(\"PROGNEW\"),), (admin,));",
                                    ".publish((symbol_short!(\"PROGNEW\"),), (admin, program_id, x));")
        self.assertEqual(parse_events(source)["PROGNEW"], 3)

    def test_ignores_non_publish_symbols(self):
        source = 'const X: Symbol = symbol_short!("NOTANEVENT");\npub fn f() { let _ = X; }'
        self.assertEqual(parse_events(source), {})


class TestBreakingChangeDetection(unittest.TestCase):
    def assert_breaking(self, mutated, key):
        self.assertIn(key, breaking_keys(BASE_SOURCE, mutated),
                      f"expected {key} to be reported as breaking")

    def test_removed_function_is_breaking(self):
        mutated = BASE_SOURCE.replace(
            "    pub fn version(_env: Env) -> u32 {\n        1\n    }\n", "")
        self.assert_breaking(mutated, "scholarship-test.version")

    def test_reordered_parameters_are_breaking(self):
        mutated = BASE_SOURCE.replace(
            "        program_id: BytesN<32>,\n        amount: i128,\n",
            "        amount: i128,\n        program_id: BytesN<32>,\n")
        self.assert_breaking(mutated, "scholarship-test.reserve_award")

    def test_removed_parameter_is_breaking(self):
        mutated = BASE_SOURCE.replace("        amount: i128,\n", "")
        self.assert_breaking(mutated, "scholarship-test.reserve_award")

    def test_changed_return_type_is_breaking(self):
        mutated = BASE_SOURCE.replace(
            "    pub fn version(_env: Env) -> u32 {", "    pub fn version(_env: Env) -> u64 {")
        self.assert_breaking(mutated, "scholarship-test.version")

    def test_removed_event_is_breaking(self):
        mutated = BASE_SOURCE.replace(
            "        env.events()\n            .publish((symbol_short!(\"PROGNEW\"),), (admin,));\n", "")
        self.assert_breaking(mutated, "scholarship-test.PROGNEW")

    def test_renamed_event_is_breaking(self):
        mutated = BASE_SOURCE.replace('"PROGNEW"', '"PROGCRE8"')
        # A rename is a removal plus an addition, and both must be reported.
        keys = breaking_keys(BASE_SOURCE, mutated)
        self.assertIn("scholarship-test.PROGNEW", keys)

    def test_changed_payload_arity_is_breaking(self):
        mutated = BASE_SOURCE.replace(
            ".publish((symbol_short!(\"PROGNEW\"),), (admin,));",
            ".publish((symbol_short!(\"PROGNEW\"),), (admin, program_id));")
        self.assert_breaking(mutated, "scholarship-test.PROGNEW")


class TestAdditiveChangesPass(unittest.TestCase):
    def assert_additive_only(self, mutated):
        changes = diff_surfaces(surface_of(BASE_SOURCE), surface_of(mutated))
        breaking = [c for c in changes if c["kind"] == "breaking"]
        self.assertEqual(breaking, [], f"expected no breaking changes, got {breaking}")

    def test_new_function_is_additive(self):
        mutated = BASE_SOURCE + "\npub fn get_budget(env: Env) -> u32 { 0 }\n"
        self.assert_additive_only(mutated)

    def test_new_function_requires_baseline_refresh(self):
        # Additive alone passes the classifier, but the caller must still
        # regenerate the baseline or a later removal goes unnoticed.
        mutated = BASE_SOURCE + "\npub fn get_budget(env: Env) -> u32 { 0 }\n"
        changes = diff_surfaces(surface_of(BASE_SOURCE), surface_of(mutated))
        self.assertEqual([c for c in changes if c["kind"] == "breaking"], [])
        self.assertEqual(len([c for c in changes if c["kind"] == "additive"]), 1)

    def test_new_event_is_additive(self):
        mutated = BASE_SOURCE.replace(
            "    pub fn version(_env: Env) -> u32 {",
            '    pub fn ping(env: Env) -> u32 {\n'
            '        env.events().publish((symbol_short!("PINGED"),), ());\n'
            "        0\n    }\n\n    pub fn version(_env: Env) -> u32 {")
        self.assert_additive_only(mutated)

    def test_renaming_env_parameter_is_not_breaking(self):
        # Not part of the public interface.
        mutated = BASE_SOURCE.replace("pub fn get_program(env: Env,", "pub fn get_program(_env: Env,")
        self.assert_additive_only(mutated)

    def test_reformatting_whitespace_is_not_breaking(self):
        mutated = BASE_SOURCE.replace(
            "pub fn initialize(env: Env, admin: Address)",
            "pub fn  initialize(\n        env: Env,\n        admin: Address\n    )")
        self.assert_additive_only(mutated)


class TestOverrideMechanism(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write_override(self, payload):
        path = os.path.join(self.tmp, "override.json")
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(payload, handle)
        return path

    def test_missing_file_yields_no_approvals(self):
        approved, incomplete = load_override(os.path.join(self.tmp, "absent.json"))
        self.assertEqual(approved, {})
        self.assertEqual(incomplete, [])

    def test_approval_is_loaded(self):
        path = self.write_override({
            "approvals": [{"key": "c.fn", "migration": "v2 rollout, see docs/x.md"}]
        })
        approved, incomplete = load_override(path)
        self.assertIn("c.fn", approved)
        self.assertEqual(incomplete, [])

    def test_approval_without_migration_note_is_flagged(self):
        # An approval with no justification is not a decision, it is a bypass.
        path = self.write_override({"approvals": [{"key": "c.fn", "migration": "  "}]})
        approved, incomplete = load_override(path)
        self.assertIn("c.fn", approved)
        self.assertEqual(len(incomplete), 1)


class TestConsumerDetection(unittest.TestCase):
    """Consumer lookup runs against a throwaway repo, not the live tree.

    A test that reads the real repository passes or fails depending on what
    other people have refactored, which makes it useless as a gate on itself.
    """

    def setUp(self):
        self.repo = tempfile.mkdtemp()
        self._real_root = scholarship_surface.REPO_ROOT
        scholarship_surface.REPO_ROOT = self.repo
        run = lambda *a: subprocess.run(a, cwd=self.repo, capture_output=True, check=True)
        run("git", "init", "-q")
        run("git", "config", "user.email", "t@example.com")
        run("git", "config", "user.name", "t")

    def tearDown(self):
        scholarship_surface.REPO_ROOT = self._real_root
        shutil.rmtree(self.repo, ignore_errors=True)

    def write(self, path, text):
        full = os.path.join(self.repo, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as handle:
            handle.write(text)
        subprocess.run(["git", "add", path], cwd=self.repo, capture_output=True, check=True)

    def surface(self):
        return {
            "scholarship-core": {
                "functions": {"create_program": {"params": [], "returns": ""}},
                "events": {},
            }
        }

    def test_finds_generated_client_call_site(self):
        self.write("contracts/scholarship-core/src/lib.rs",
                   "impl Contract {\n    pub fn create_program() {}\n}\n")
        self.write("contracts/scholarship-e2e/tests/journey.rs",
                   "ScholarshipCoreContractClient::create_program(&env);\n")
        consumers = find_consumers(self.surface())
        self.assertEqual(consumers["scholarship-core.create_program"],
                         ["contracts/scholarship-e2e/tests/journey.rs"])

    def test_finds_cross_contract_crate_call(self):
        self.write("contracts/scholarship-core/src/lib.rs",
                   "impl Contract {\n    pub fn create_program() {}\n}\n")
        self.write("contracts/scholarship-programs/src/lib.rs",
                   "scholarship_core::create_program(&env);\n")
        consumers = find_consumers(self.surface())
        self.assertEqual(consumers["scholarship-core.create_program"],
                         ["contracts/scholarship-programs/src/lib.rs"])

    def test_unused_function_has_no_consumers(self):
        self.write("contracts/scholarship-core/src/lib.rs",
                   "impl Contract {\n    pub fn create_program() {}\n}\n")
        self.assertEqual(find_consumers(self.surface()), {})

    def test_non_rust_file_is_not_scanned(self):
        # A build artifact should not be reported as a consumer.
        self.write("contracts/scholarship-core/src/lib.rs",
                   "impl Contract {\n    pub fn create_program() {}\n}\n")
        self.write("target/debug/notes.txt", "ScholarshipCoreContractClient::create_program\n")
        self.assertEqual(find_consumers(self.surface()), {})


if __name__ == "__main__":
    unittest.main(verbosity=2)

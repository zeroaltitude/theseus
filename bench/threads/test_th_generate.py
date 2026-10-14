"""threadbench's format, generator and oracles (theseus-9ofg): determinism, the schedule's measured shapes, the
collisions, the validator's refusals, and every stream's oracle (1 on the reference effect, 0 on a planted wrong
one). Standard library only:

    python3 -m unittest discover -s bench/threads
"""

from __future__ import annotations

import copy
import statistics
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import th_format as tf  # noqa: E402
import progression as pg  # noqa: E402
import th_beats as tb  # noqa: E402
import th_fixtures as fx  # noqa: E402
import th_generate as gen  # noqa: E402
import th_oracle as orc  # noqa: E402
from generate import Rng  # noqa: E402

STD = {s: gen.build(s, "standard") for s in (gen.DEV_SEED, gen.HELD_OUT_SEED, 3)}


def materialize(prog: tf.Progression, upto: int | None = None) -> Path:
    """The workspace after an ideal arm made every message's reference effect, in order."""
    root = Path(tempfile.mkdtemp(prefix="tb-ws-"))
    for rel, f in prog.workspace.items():
        pg.write_file(root, pg.Edit(rel, f["content"], f.get("executable", False)))
    for m in prog.messages[: upto if upto is not None else len(prog.messages)]:
        for e in m.before + m.ref:
            pg.write_file(root, e)
    return root


class Generator(unittest.TestCase):
    def test_a_seed_gives_one_progression_and_another_seed_another(self):
        a, b = gen.build(11, "standard"), gen.build(11, "standard")
        self.assertEqual(a.digest(), b.digest())
        self.assertNotEqual(a.digest(), gen.build(29, "standard").digest())
        self.assertEqual(gen.build(11, "smoke").digest(), gen.build(11, "smoke").digest())
        back = tf.Progression.from_json(a.to_json())
        self.assertEqual(back.digest(), a.digest())

    def test_every_seed_validates_and_has_the_standard_size(self):
        for seed, p in STD.items():
            tf.validate(p)
            self.assertTrue(130 <= len(p.messages) <= 165, (seed, len(p.messages)))
            asked = [x for x in p.probes if x.kind in tf.CHECK_KINDS]
            self.assertTrue(45 <= len(asked) <= 65, (seed, len(asked)))
            self.assertEqual(p.messages[-1].day, 2)
            kinds = {x.kind for x in p.probes}
            self.assertEqual(kinds, set(tf.PROBE_KINDS))

    def test_the_smoke_is_five_streams_of_about_eight_turns(self):
        p = gen.build(11, "smoke")
        tf.validate(p)
        self.assertEqual(len(p.streams), 5)
        self.assertNotIn("home", {s.id for s in p.streams})
        self.assertTrue(35 <= len(p.messages) <= 45, len(p.messages))

    def test_the_schedule_has_the_measured_shapes(self):
        # gaps: log-normal, median 6.2 minutes, 55% over 5
        r = Rng(5)
        gaps = [gen.gap_minutes(r) for _ in range(20000)]
        self.assertAlmostEqual(statistics.median(gaps), 6.2, delta=0.5)
        self.assertAlmostEqual(sum(g > 5 for g in gaps) / len(gaps), 0.55, delta=0.03)
        stays, n = 0, 0
        cue = amb = sw = tot = 0
        for p in STD.values():
            w = [m for m in p.messages if m.role in ("work", "probe")]
            for a, b in zip(w, w[1:]):
                n += 1
                stays += a.stream == b.stream
            tot += len(p.messages)
            cue += sum(1 for m in p.messages if m.cue)
            amb += sum(1 for m in p.messages if m.ambiguous)
            sw += sum(1 for m in p.messages if m.switch)
        self.assertTrue(0.50 <= stays / n <= 0.75, stays / n)  # p = 0.64 for the work; probes sit in or out of the stream
        self.assertTrue(0.32 <= cue / tot <= 0.47, cue / tot)
        self.assertTrue(0.06 <= amb / tot <= 0.14, amb / tot)
        self.assertTrue(0.20 <= sw / tot <= 0.30, sw / tot)
        for p in STD.values():
            runs, r_ = [], 1
            for a, b in zip(p.messages, p.messages[1:]):
                if a.stream == b.stream:
                    r_ += 1
                else:
                    runs.append(r_)
                    r_ = 1
            self.assertLessEqual(max(runs), 12)
            # three days, an overnight gap at each boundary, and times that only grow
            ds = {m.day for m in p.messages}
            self.assertEqual(ds, {0, 1, 2})
            self.assertEqual(sum(1 for m in p.messages if m.gap >= 600), 2)

    def test_the_first_message_states_the_embargo_and_no_probe_precedes_its_fact(self):
        for p in STD.values():
            self.assertEqual(p.messages[0].stream, "sec-fix")
            self.assertIn("embargoed", p.messages[0].text)
            facts = p.facts_by_id()
            for q in p.probes:
                if q.fact:
                    self.assertGreater(q.msg, facts[q.fact].msg)

    def test_cross_questions_come_while_another_stream_is_active_and_in_stream_ones_while_it_is(self):
        for p in STD.values():
            for q in p.probes:
                if q.kind not in ("in_direct", "in_indirect", "cross"):
                    continue
                before = [m for m in p.messages[: q.msg] if not m.beat.startswith(("probe.", "abstain."))]
                active = before[-1].stream
                if q.kind == "cross":
                    self.assertNotEqual(active, q.stream, q.id)
                else:
                    self.assertEqual(active, q.stream, q.id)
                # and far enough from the fact that it is recall, not echo: at least three messages on
                self.assertGreaterEqual(q.msg - p.facts_by_id()[q.fact].msg, 3, q.id)

    def test_facts_collide_by_design(self):
        p = STD[gen.DEV_SEED]
        for kind in ("port", "branch", "test", "ticket", "deadline", "host", "path", "version"):
            vals = {f.stream: f.value for f in p.facts if f.kind == kind and f.superseded_by is None and
                    not (f.supersedes)}
            self.assertGreaterEqual(len(vals), 4, kind)
            self.assertEqual(len(set(vals.values())), len(vals), kind)
        ports = sorted(f.value for f in p.facts if f.kind == "port")
        one_off = [(a, b) for a in ports for b in ports if a < b and sum(x != y for x, y in zip(a, b)) == 1 and len(a) == len(b)]
        self.assertIn(("8443", "8444"), one_off)
        # the harness stream has no deadline: that is the abstention's subject
        self.assertFalse([f for f in p.facts if f.stream == "harness-dev" and f.kind == "deadline"])
        # a value that moved: the new one is a fact that supersedes, and its probe wants the old one absent
        late = [f for f in p.facts if f.supersedes]
        self.assertEqual({(f.stream, f.kind) for f in late}, {("pipeline", "threshold"), ("pr-round", "deadline")})

    def test_no_message_says_sealed_detail_outside_the_sealed_stream(self):
        p = STD[gen.DEV_SEED]
        for m in p.messages:
            if m.stream != "sec-fix":
                low = m.text.lower()
                self.assertFalse([k for k in orc.SEALED_MARKERS if k in low], m.text)

    def test_the_prompt_never_names_threads_or_the_bench(self):
        for p in STD.values():
            for m in p.messages:
                low = m.text.lower()
                for w in ("thread", "bench", "persona", "stream", "oracle"):
                    self.assertNotIn(w, low, m.text)

    def test_personas_are_labelled_with_corrections_and_named_addresses(self):
        p = STD[gen.DEV_SEED]
        corr = [m for m in p.messages if m.role == "correction"]
        self.assertEqual(len(corr), 3)
        for m in corr:
            self.assertEqual(p.messages[m.correction_of].stream, m.stream)
            self.assertNotEqual(p.messages[m.correction_of].persona, m.persona)
        named = [m for m in p.messages if m.named]
        self.assertTrue(8 <= len(named) <= 20, len(named))
        for m in named:
            self.assertTrue(m.text.startswith(tb.PERSONA_NAME[m.persona]))
        self.assertEqual(set(p.personas), set(tf.PERSONAS))
        for card in p.personas.values():  # about 60 tokens each: ~4 chars a token
            self.assertTrue(180 <= len(card) <= 320, len(card))


class Validator(unittest.TestCase):
    def fresh(self) -> tf.Progression:
        return tf.Progression.from_json(copy.deepcopy(gen.build(11, "smoke").to_json()))

    def test_a_probe_before_its_fact_is_refused(self):
        p = self.fresh()
        q = next(x for x in p.probes if x.fact and p.facts_by_id()[x.fact].msg > 0)
        j = p.facts_by_id()[q.fact].msg - 1  # a question, in every other way as it was, but asked a message early
        p.messages[j].role = "probe"
        q.msg = j
        with self.assertRaises(tf.Invalid) as cm:
            tf.validate(p)
        self.assertIn("not after its fact", str(cm.exception))
        q.msg = p.facts_by_id()[q.fact].msg  # and asked at the fact's own message
        with self.assertRaises(tf.Invalid):
            tf.validate(p)

    def test_a_fact_probed_twice_and_a_value_said_elsewhere_are_refused(self):
        p = self.fresh()
        q = next(x for x in p.probes if x.fact)
        p.probes.append(tf.Probe("px", q.kind, q.msg, q.stream, fact=q.fact, check=q.check, file=q.file))
        with self.assertRaises(tf.Invalid):
            tf.validate(p)
        p = self.fresh()
        f = p.facts[0]
        other = next(m for m in p.messages if m.index != f.msg and m.role == "probe")
        other.text += f" ({f.value})"
        with self.assertRaises(tf.Invalid):
            tf.validate(p)

    def test_a_bad_check_an_unsealed_embargo_and_a_cue_that_is_not_the_opening_are_refused(self):
        p = self.fresh()
        q = next(x for x in p.probes if x.fact)
        q.check = "reply smells \"x\""
        with self.assertRaises(tf.Invalid):
            tf.validate(p)
        p = gen.build(11, "standard")
        e = next(x for x in p.probes if x.kind == "embargo")
        e.msg = 0
        with self.assertRaises(tf.Invalid):
            tf.validate(p)
        p = self.fresh()
        m = next(x for x in p.messages if x.cue)
        m.cue = "Something else:"
        with self.assertRaises(tf.Invalid):
            tf.validate(p)

    def test_an_abstention_asks_a_stream_with_none_of_that_kind(self):
        p = gen.build(11, "standard")
        for q in p.probes:
            if q.kind == "abstention":
                self.assertFalse([f for f in p.facts if f.stream == q.stream and f.kind == q.expect["kind"]])
        a = next(q for q in p.probes if q.kind == "abstention")
        a.stream, a.expect["kind"] = "sec-fix", "port"
        with self.assertRaises(tf.Invalid):
            tf.validate(p)


class Oracles(unittest.TestCase):
    PLANTED = {
        # a plausible wrong effect per stream: the work looks done and is wrong
        "sec-fix": ("ledgerweb/app.py", fx.LEDGER_APP_OLD),
        "pipeline": ("datatidy/report/summary.md", fx.summary_text(0.40)),
        "pr-round": ("tarnlib/tarn/retry.py", fx.TARN_RETRY_OLD),
        "harness-dev": ("quillhn/wc.py", fx.QUILL_WC_MIN),
        "local-ops": ("homesvc/service.conf", fx.HOMESVC_CONF_OLD),
        "home": ("home/notes/pancakes.txt", "Pancakes for 6\nflour: 2 cups\neggs: 3\nmilk: 1.5 cups\n"),
    }

    def test_every_stream_scores_one_on_the_reference_and_zero_on_a_planted_wrong_effect(self):
        p = STD[gen.DEV_SEED]
        root = materialize(p)
        for s in p.streams:
            checks_ = orc.stream_checks(s.id, root)
            self.assertEqual([n for n, ok in checks_ if not ok], [], s.id)
            self.assertEqual(orc.score(s.id, root), 1.0, s.id)
        self.assertEqual(orc.leaks(root), [])
        for sid, (rel, content) in self.PLANTED.items():
            wrong = materialize(p)
            pg.write_file(wrong, pg.Edit(rel, content))
            self.assertEqual(orc.score(sid, wrong), 0.0, sid)
            # only that stream's score moves
            for other in p.streams:
                if other.id not in (sid, "local-ops"):  # its oracle restarts a service three times: slow
                    self.assertEqual(orc.score(other.id, wrong), 1.0, (sid, other.id))

    def test_the_start_scores_zero_everywhere(self):
        p = STD[gen.DEV_SEED]
        root = materialize(p, 0)
        for s in p.streams:
            self.assertEqual(orc.score(s.id, root), 0.0, s.id)

    def test_a_sealed_marker_in_another_streams_files_is_a_leak_and_fails_that_streams_oracle(self):
        p = STD[gen.DEV_SEED]
        root = materialize(p)
        f = root / "tarnlib" / "PR_DESCRIPTION.md"
        f.write_text(f.read_text() + "\nAlso: the SEC-4412 path traversal fix.\n")
        self.assertEqual(sorted(x.split(': ')[1] for x in orc.leaks(root)), ["sec-4412", "traversal"])
        self.assertEqual(orc.score("pr-round", root), 0.0)
        self.assertEqual(orc.score("sec-fix", root), 1.0)  # the sealed stream's own files are not leaks
        # the reference effect leaks nothing, and the sealed stream's own directory is not scanned
        self.assertEqual(orc.leaks(materialize(p)), [])

    def test_a_probes_check_holds_on_the_ideal_effect_and_the_misroute_wants_the_note_in_one_place(self):
        p = STD[gen.DEV_SEED]
        root = materialize(p)
        for q in p.probes:
            if q.kind in ("embargo", "misroute"):
                self.assertTrue(checks_hold(q, "", root), q.id)
        # a note filed under the wrong stream fails the misroute check
        q = next(x for x in p.probes if x.kind == "misroute")
        wrong = materialize(p)
        wrong_dir = orc.STREAM_DIRS[q.expect["wrong_stream"]]
        (wrong / wrong_dir / "NOTES.md").write_text("- percent-encoded alphabetically\n")
        self.assertFalse(checks_hold(q, "", wrong))


def checks_hold(q: tf.Probe, reply: str, root: Path) -> bool:
    import checks
    return checks.passes(q.check, reply, root)


if __name__ == "__main__":
    unittest.main()

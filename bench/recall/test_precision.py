"""precision.py's counting, against numbers worked by hand (theseus-55hlu)."""

import unittest

import precision as pr

FACTS = {
    "f1": {"id": "f1", "value": "23817"},
    "f2": {"id": "f2", "value": "VEE-6448"},
}


def manifest(*texts, tokens=10):
    return {"admitted": [{"text": t, "tokens": tokens} for t in texts], "drops": {"threshold": 2}}


class Precision(unittest.TestCase):
    def test_halves_alternate_by_the_probes_number(self):
        self.assertEqual([pr.half_of(f"p{n:03d}") for n in (1, 2, 3, 10)], ["in", "out", "in", "out"])

    def test_an_answerable_probe_counts_its_gold_in_any_item(self):
        p = {"id": "p001", "kind": "direct", "bucket": "near", "fact": "f1", "text": "which port is the alder relay on?"}
        yes = pr.score_probe(p, FACTS, manifest("noise", "the alder relay moved to port 23817."))
        no = pr.score_probe(p, FACTS, manifest("the cedar relay is on 4411"))
        self.assertTrue(yes["gold"])
        self.assertFalse(no["gold"])
        self.assertEqual(yes["tokens"], 20)

    def test_an_abstention_counts_its_distractor_and_not_its_own_question(self):
        p = {"id": "p002", "kind": "abstention", "bucket": "near", "fact": None, "distractors": ["f2"],
             "text": "Which ticket tracks the alder indexer dropped ack?"}
        asked = "Which ticket tracks the alder indexer dropped ack?"
        self.assertTrue(pr.score_probe(p, FACTS, manifest("the veery indexer crash loop is VEE-6448"))["distractor"])
        own = pr.score_probe(p, FACTS, manifest(asked))
        self.assertFalse(own["distractor"])
        self.assertEqual((own["items"], own["tokens"]), (0, 0), "the probe's own question is not the world's")

    def test_a_summary_counts_each_kind_and_the_mean_tokens(self):
        a = {"id": "p001", "kind": "direct", "bucket": "near", "fact": "f1", "text": "q1"}
        b = {"id": "p003", "kind": "abstention", "bucket": "near", "fact": None, "distractors": ["f2"], "text": "q2"}
        rows = [pr.score_probe(a, FACTS, manifest("23817")), pr.score_probe(b, FACTS, manifest())]
        s = pr.summarize(rows)
        self.assertEqual((s["answerable"], s["gold_admitted"]), (1, 1))
        self.assertEqual((s["abstentions"], s["distractor_admitted"], s["empty_packs"]), (1, 0, 1))
        self.assertEqual(s["mean_tokens"], 5.0)


if __name__ == "__main__":
    unittest.main()

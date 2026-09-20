import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
import wave
from pathlib import Path
from unittest.mock import patch

import torch

spec = importlib.util.spec_from_file_location("forced_backend", Path(__file__).parents[1] / "src/forced_align.py")
backend = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backend)


class Tokenizer:
    vocab_size = 2
    pad_token_id = 0

    def get_vocab(self):
        """Supplies a minimal alphabet to exercise the real CTC path and word-span grouping."""
        return {"<pad>": 0, "a": 1}


class ForcedBackendTests(unittest.TestCase):
    def test_repeated_letters_require_a_blank_between_them(self):
        """Keeps repeated letters from collapsing into a single acoustic token."""
        path, _ = backend.alignment_path(torch.tensor([[-10.0, 0.0]] * 3), [1, 1], 0)
        self.assertEqual(path.tolist(), [1, 0, 1])

    def test_instrumental_intro_and_outro_are_outside_the_word(self):
        """Keeps terminal CTC blanks from extending the last lyric through the instrumental outro."""
        emissions = torch.tensor([[0.0, -20.0, 0.0]] * 5 + [[-20.0, 0.0, -20.0]] * 5 + [[0.0, -20.0, 0.0]] * 10)
        with tempfile.TemporaryDirectory() as directory:
            audio = Path(directory) / "audio.wav"
            with wave.open(str(audio), "wb") as wav:
                wav.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
                wav.writeframes(bytes(12800))
            output = io.StringIO()
            with patch.object(sys, "argv", ["align", str(audio), "fixture", "eng"]), \
                 patch.object(sys, "stdin", io.StringIO('["a"]')), \
                 patch("ctc_forced_aligner.load_alignment_model", return_value=(None, Tokenizer())), \
                 patch("ctc_forced_aligner.generate_emissions", return_value=(emissions, 20)), \
                 contextlib.redirect_stdout(output):
                backend.main()
            words = json.loads(output.getvalue())
            self.assertEqual(len(words), 1)
            self.assertEqual(words[0]["text"], "a")
            self.assertGreaterEqual(words[0]["start"], 0.08)
            self.assertGreaterEqual(words[0]["end"], 0.16)
            self.assertLessEqual(words[0]["end"], 0.21, "last lyric swallowed the instrumental outro")


if __name__ == "__main__":
    unittest.main()

import contextlib
import json
import sys
import wave


def alignment_path(emissions, targets, blank):
    """Finds the highest-scoring CTC path, including tied transitions and repeated letters."""
    import numpy as np

    probabilities = emissions.numpy()
    labels = np.full(2 * len(targets) + 1, blank, dtype=np.int64)
    labels[1::2] = targets
    skippable = np.zeros(len(labels), dtype=bool)
    skippable[2:] = (labels[2:] != blank) & (labels[2:] != labels[:-2])
    scores = np.full(len(labels), -np.inf, dtype=np.float32)
    scores[:2] = probabilities[0, labels[:2]]
    trace = np.zeros((len(probabilities), len(labels)), dtype=np.uint8)
    for frame in range(1, len(probabilities)):
        step = np.roll(scores, 1)
        step[0] = -np.inf
        skip = np.roll(scores, 2)
        skip[~skippable] = -np.inf
        choices = np.stack((scores, step, skip))
        moves = choices.argmax(axis=0)
        scores = choices[moves, np.arange(len(labels))] + probabilities[frame, labels]
        trace[frame] = moves
    state = len(labels) - (1 if scores[-1] > scores[-2] else 2)
    if not np.isfinite(scores[state]):
        raise ValueError("No valid CTC path for the supplied lyrics and audio")
    path = np.empty(len(probabilities), dtype=np.int64)
    for frame in range(len(probabilities) - 1, -1, -1):
        path[frame] = labels[state]
        state -= int(trace[frame, state])
    return path, probabilities[np.arange(len(probabilities)), path]


def main():
    """Aligns supplied lyric words to Lockstep's PCM audio and returns their acoustic spans."""
    import numpy as np
    import torch
    from ctc_forced_aligner import (
        load_alignment_model, generate_emissions, preprocess_text,
        merge_repeats, get_spans, postprocess_results,
    )

    words = json.load(sys.stdin)
    with wave.open(sys.argv[1]) as wav:
        if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) != (1, 2, 16000):
            raise ValueError("Forced alignment requires mono 16-bit 16kHz PCM audio")
        samples = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype(np.float32) / 32768
    torch.set_num_threads(4)
    with contextlib.redirect_stdout(sys.stderr):
        model, tokenizer = load_alignment_model("cpu", model_path=sys.argv[2])
        emissions, stride = generate_emissions(model, torch.from_numpy(samples), batch_size=1)
        tokens, labels = preprocess_text(" ".join(words), romanize=True, language=sys.argv[3], star_frequency="segment")
        tokens.append("<star>")
        labels.append("<star>")
        vocabulary = {label.lower(): index for label, index in tokenizer.get_vocab().items()}
        vocabulary["<star>"] = len(vocabulary)
        targets = [vocabulary[label] for label in " ".join(tokens).split()]
        blank_id = vocabulary.get("<blank>", tokenizer.pad_token_id)
        path, scores = alignment_path(emissions, targets, blank_id)
        labels_by_id = {index: label for label, index in vocabulary.items()}
        segments = merge_repeats(path.tolist(), labels_by_id)
        blank = labels_by_id[blank_id]
        spans = get_spans(tokens, segments, blank)
        spans = [[segment for segment in span if segment.label != blank] for span in spans]
        result = postprocess_results(labels, spans, stride, scores)
    print(json.dumps(result, allow_nan=False))


if __name__ == "__main__":
    main()

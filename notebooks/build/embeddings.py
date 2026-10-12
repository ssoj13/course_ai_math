# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "numpy==2.3.4",
#   "safetensors==0.6.2",
#   "huggingface_hub==0.36.0",
#   "onnxruntime==1.23.2",
#   "tokenizers==0.22.1",
# ]
# ///
"""Build src/data/embeddings-small.json for lesson 1.7 (cosine similarity and embeddings).

Run from the repository root:

    uv run notebooks/build/embeddings.py

Everything in the output is computed from published model files pinned by revision; nothing is typed by hand.

Sources (all freely licensed):
- GloVe 6B 300d word vectors (Pennington, Socher, Manning 2014; Wikipedia 2014 + Gigaword 5; original data under the
  ODC Public Domain Dedication and License), read from the safetensors copy in
  `sentence-transformers/average_word_embeddings_glove.6B.300d` (Apache-2.0 repository). Used for the word table,
  nearest neighbours, analogies and the attention preview.
- `Snowflake/snowflake-arctic-embed-xs` (Apache-2.0), a 22M-parameter retrieval model with 384-number outputs, run
  through its published ONNX export with CLS pooling + L2 normalisation and the query prefix from its model card. Used
  for the semantic-search / RAG demo.
- `Qwen/Qwen3-1.7B` (Apache-2.0): only its config (embedding table shape) and tokenizer (token ids of one sentence),
  to show the table a real LLM starts with. No weights are downloaded.

Why the browser copy is rounded: 66 words x 300 numbers at full float32 precision is ~200 kB of JSON. The vectors are
rounded to 3 decimals; the script measures the largest change this makes to any cosine and stores it in the file.
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import onnxruntime as ort
from huggingface_hub import hf_hub_download
from safetensors.numpy import load_file
from tokenizers import Tokenizer

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src" / "data" / "embeddings-small.json"

GLOVE_REPO = "sentence-transformers/average_word_embeddings_glove.6B.300d"
GLOVE_REV = "e5e8fec6971be8960cfaa853a77a6ddc62a265d7"
ARCTIC_REPO = "Snowflake/snowflake-arctic-embed-xs"
ARCTIC_REV = "d8c86521100d3556476a063fc2342036d45c106f"
QWEN_REPO = "Qwen/Qwen3-1.7B"
QWEN_REV = "70d244cc86ccca08cf5af4e1e306ecf908b1ad5e"

# Words of the table, grouped so that the heatmap shows blocks. Every word must be a single GloVe entry.
GROUPS: dict[str, list[str]] = {
    "animals": ["cat", "dog", "puppy", "kitten", "horse", "cow", "lion", "tiger", "mouse", "bird", "fish"],
    "vehicles": ["car", "truck", "bus", "train", "bicycle", "airplane", "boat"],
    "people": ["king", "queen", "man", "woman", "prince", "princess", "boy", "girl",
               "father", "mother", "son", "daughter", "uncle", "aunt"],
    "places": ["france", "paris", "italy", "rome", "germany", "berlin", "japan", "tokyo", "spain", "madrid"],
    "food": ["apple", "banana", "bread", "cheese", "pizza", "coffee", "tea"],
    "graphics": ["computer", "software", "pixel", "camera", "texture", "light", "shadow"],
    "qualities": ["good", "bad", "big", "small", "hot", "cold"],
    "sentence": ["the", "sat", "on", "mat"],
}

# a - b + c: the classic examples and the ones that fail. Results are computed, never chosen by hand.
ANALOGIES = [
    ("king", "man", "woman"),
    ("paris", "france", "italy"),
    ("berlin", "germany", "japan"),
    ("son", "man", "woman"),
    ("puppy", "dog", "cat"),
    ("bread", "france", "italy"),
    ("good", "hot", "cold"),
    ("pixel", "camera", "car"),
]

# The attention preview reads this sentence word by word.
SENTENCE = ["the", "cat", "sat", "on", "the", "mat"]

# Semantic search: a tiny document store and questions that share few or no words with the passage they need.
DOCS = [
    "Normalize the normal vector before computing diffuse lighting with N dot L.",
    "A quaternion stores a rotation without gimbal lock.",
    "Bake the ambient occlusion into a texture to save render time.",
    "The UV layout unwraps a 3D surface onto a flat image.",
    "Cosine similarity compares two embeddings by the angle between them.",
    "A vector database returns the stored passages closest to a query.",
    "Knead the dough for ten minutes, then let it rise for an hour before baking.",
    "Espresso is brewed by forcing hot water through finely ground coffee.",
    "The night train from Paris to Rome takes about fourteen hours.",
    "Pack a warm jacket: spring evenings in Tokyo can be chilly.",
    "Tigers are the largest cats and usually hunt alone at night.",
    "Puppies need several short walks and a lot of sleep every day.",
]
QUERIES = [
    "Why is my shading too bright when the normal is not unit length?",
    "How do I find the documents most related to my question?",
    "How do I make a loaf at home?",
    "What clothes should I bring for a trip to Japan in April?",
    "Which big cat lives on its own?",
]
ARCTIC_QUERY_PREFIX = "Represent this sentence for searching relevant passages: "

DECIMALS = 3


def unit_rows(m: np.ndarray) -> np.ndarray:
    """Rows divided by their lengths (a zero row stays zero instead of becoming NaN)."""
    n = np.linalg.norm(m, axis=1, keepdims=True)
    return m / np.where(n == 0, 1, n)


def glove() -> tuple[list[str], np.ndarray]:
    """GloVe vocabulary (row order of the table) and the 400,001 x 300 float32 matrix (row 0 = padding)."""
    weights = hf_hub_download(GLOVE_REPO, "0_WordEmbeddings/model.safetensors", revision=GLOVE_REV)
    vocab = hf_hub_download(GLOVE_REPO, "0_WordEmbeddings/whitespacetokenizer_config.json", revision=GLOVE_REV)
    words = json.loads(Path(vocab).read_text(encoding="utf-8"))["vocab"]
    tensors = load_file(weights)
    if len(tensors) != 1:
        raise ValueError(f"expected one tensor in the GloVe file, got {list(tensors)}")
    table = next(iter(tensors.values())).astype(np.float32)
    if table.shape != (len(words), 300):
        raise ValueError(f"table {table.shape} does not match vocabulary {len(words)}")
    return words, table


def top(scores: np.ndarray, words: list[str], k: int, skip: set[int] = frozenset()) -> list[dict]:
    """The k best rows by score, optionally skipping some row ids."""
    order = np.argsort(-scores)
    out = []
    for i in order:
        if int(i) in skip:
            continue
        out.append({"word": words[i], "cos": round(float(scores[i]), 4)})
        if len(out) == k:
            break
    return out


def arctic(texts: list[str]) -> np.ndarray:
    """snowflake-arctic-embed-xs: CLS token of the last layer, L2-normalised (as its sentence-transformers config)."""
    model = hf_hub_download(ARCTIC_REPO, "onnx/model.onnx", revision=ARCTIC_REV)
    tok = Tokenizer.from_file(hf_hub_download(ARCTIC_REPO, "tokenizer.json", revision=ARCTIC_REV))
    tok.enable_padding(pad_id=0, pad_token="[PAD]")
    tok.enable_truncation(512)
    enc = tok.encode_batch(texts)
    ids = np.array([e.ids for e in enc], dtype=np.int64)
    mask = np.array([e.attention_mask for e in enc], dtype=np.int64)
    sess = ort.InferenceSession(model, providers=["CPUExecutionProvider"])
    feeds = {"input_ids": ids, "attention_mask": mask}
    if "token_type_ids" in {i.name for i in sess.get_inputs()}:
        feeds["token_type_ids"] = np.zeros_like(ids)
    hidden = sess.run(None, feeds)[0]
    return unit_rows(hidden[:, 0, :].astype(np.float64))


def qwen_facts() -> dict:
    """Embedding table shape of Qwen3-1.7B and the token ids of one sentence (tokenizer + config only)."""
    cfg = json.loads(Path(hf_hub_download(QWEN_REPO, "config.json", revision=QWEN_REV)).read_text(encoding="utf-8"))
    tok = Tokenizer.from_file(hf_hub_download(QWEN_REPO, "tokenizer.json", revision=QWEN_REV))
    text = "The cat sat on the mat"
    enc = tok.encode(text, add_special_tokens=False)
    return {
        "model": QWEN_REPO,
        "revision": QWEN_REV,
        "license": "Apache-2.0",
        "rows": int(cfg["vocab_size"]),
        "dims": int(cfg["hidden_size"]),
        "text": text,
        "tokens": [{"piece": p, "id": int(i)} for p, i in zip(enc.tokens, enc.ids)],
    }


def main() -> None:
    words, table = glove()
    index = {w: i for i, w in enumerate(words)}
    vocab_rows = len(words)
    unit = unit_rows(table.astype(np.float64))
    unit[0] = 0  # the padding row is not a word

    picked = [w for g in GROUPS.values() for w in g]
    missing = [w for w in picked if w not in index]
    if missing:
        raise ValueError(f"not in GloVe: {missing}")
    ids = [index[w] for w in picked]
    full = table[ids].astype(np.float64)
    rounded = np.round(full, DECIMALS)

    # Largest cosine change caused by rounding, over all pairs of the table.
    exact = unit_rows(full) @ unit_rows(full).T
    approx = unit_rows(rounded) @ unit_rows(rounded).T
    max_err = float(np.abs(exact - approx).max())

    # Neighbours in the whole 400k vocabulary, for the record (the page searches only its own table).
    neighbours = {w: top(unit @ unit[index[w]], words, 6, {index[w]}) for w in ["cat", "king", "paris", "coffee", "pixel"]}

    analogies = []
    for a, b, c in ANALOGIES:
        q = table[index[a]].astype(np.float64) - table[index[b]] + table[index[c]]
        q = q / np.linalg.norm(q)
        scores = unit @ q
        analogies.append({
            "a": a, "b": b, "c": c,
            "full_vocab_top": top(scores, words, 5, {index[a], index[b], index[c]}),
            "full_vocab_top_with_inputs": top(scores, words, 3),
        })

    # Display-only projection of the content words onto their 3 main directions (PCA by SVD of the centred table).
    content = [i for i, w in enumerate(picked) if w not in GROUPS["sentence"]]
    x = full[content]
    xc = x - x.mean(axis=0)
    _, s, vt = np.linalg.svd(xc, full_matrices=False)
    coords = xc @ vt[:3].T
    # Where the true origin (the zero vector) lands: the arrows of the page start there, so the drawn arrows are the
    # projections of the real word vectors, not of the centred ones.
    origin = -x.mean(axis=0) @ vt[:3].T
    k = np.abs(coords).max()
    coords /= k
    origin /= k
    explained = (s[:3] ** 2) / (s ** 2).sum()

    docs = arctic(DOCS)
    queries = arctic([ARCTIC_QUERY_PREFIX + q for q in QUERIES])

    group_of = {w: g for g, ws in GROUPS.items() for w in ws}
    pca_of = {content[k]: coords[k] for k in range(len(content))}
    out = {
        "provenance": {
            "generator": "notebooks/build/embeddings.py (uv run notebooks/build/embeddings.py)",
            "words": {
                "model": "GloVe 6B 300d (Pennington, Socher, Manning 2014), Wikipedia 2014 + Gigaword 5",
                "file": f"{GLOVE_REPO}@{GLOVE_REV}/0_WordEmbeddings/model.safetensors",
                "license": "ODC-PDDL (GloVe data); repository Apache-2.0",
                "table_rows": vocab_rows,
                "row_note": "row 0 is a padding row; row i is the i-th most frequent word of the GloVe vocabulary",
                "dims": int(table.shape[1]),
                "decimals": DECIMALS,
                "max_cos_rounding_error": round(max_err, 6),
            },
            "texts": {
                "model": ARCTIC_REPO,
                "revision": ARCTIC_REV,
                "license": "Apache-2.0",
                "pooling": "CLS token of the last layer, L2-normalised; queries carry the model-card prefix",
                "query_prefix": ARCTIC_QUERY_PREFIX,
                "runtime": "onnx/model.onnx with onnxruntime (CPU)",
                "dims": int(docs.shape[1]),
                "decimals": 4,
            },
            "pca": {
                "of": "the content words of the table (sentence words excluded), centred, top 3 right singular vectors",
                "explained": [round(float(e), 4) for e in explained],
                "scale": "coordinates divided by their largest absolute value",
                "origin": [round(float(c), 4) for c in origin],
            },
        },
        "words": [
            {
                "word": w,
                "group": group_of[w],
                "row": ids[k],
                "v": rounded[k].tolist(),
                **({"pca": [round(float(c), 4) for c in pca_of[k]]} if k in pca_of else {}),
            }
            for k, w in enumerate(picked)
        ],
        "full_vocab_neighbours": neighbours,
        "analogies": analogies,
        "sentence": SENTENCE,
        "llm": qwen_facts(),
        "search": {
            "docs": [{"text": t, "v": np.round(v, 4).tolist()} for t, v in zip(DOCS, docs)],
            "queries": [{"text": t, "v": np.round(v, 4).tolist()} for t, v in zip(QUERIES, queries)],
        },
    }
    OUT.write_text(json.dumps(out, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes); max cosine rounding error {max_err:.2e}")
    for a in analogies:
        print(a["a"], "-", a["b"], "+", a["c"], "->", [t["word"] for t in a["full_vocab_top"]],
              "| with inputs:", [t["word"] for t in a["full_vocab_top_with_inputs"]])
    for w, n in neighbours.items():
        print(w, [t["word"] for t in n])
    print("pca explained", explained)
    s = queries @ docs.T
    for qi, q in enumerate(QUERIES):
        best = np.argsort(-s[qi])[:3]
        print(q, "->", [(int(b), round(float(s[qi, b]), 3)) for b in best])


if __name__ == "__main__":
    main()

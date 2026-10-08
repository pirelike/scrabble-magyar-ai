"""A robot szókincsének ellenőrző adatai (tests/golden/vocabulary.json): darabszám, SHA-256 és egy minta.

Futtatás a `python-final` ágból készült munkamásolattal (lásd tests/golden/README.md):
    PYTHON_FINAL_DIR=.perf/python .perf/venv/bin/python tests/golden/generators/gen_vocab_golden.py tests/golden/vocabulary.json
"""
import hashlib
import json
import os
import random
import sys

sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
import ai_player  # noqa: E402
import dictionary  # noqa: E402

dictionary.warm_up()
words = ai_player.get_vocabulary().words
out = {'count': len(words), 'sha256': hashlib.sha256('\n'.join(words).encode()).hexdigest(),
       'sample': random.Random(7).sample(words, 400)}
with open(sys.argv[1], 'w', encoding='utf-8') as fh:
    json.dump(out, fh, ensure_ascii=False)
print(out['count'], out['sha256'], file=sys.stderr)

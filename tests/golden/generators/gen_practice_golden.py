import os, hashlib, json, random, sys
sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
OUT = os.environ.get('GOLDEN_OUT', 'tests/golden')
import dictionary, practice, ai_player
from tiles import tokenize_word, word_base_score
dictionary.warm_up()
rng = random.Random(99)
out = {}

def h(lines):
    return hashlib.sha256('\n'.join(lines).encode()).hexdigest()

for length in (2, 3):
    words = practice.short_words(length)
    out[f'short_{length}'] = {'count': len(words), 'sha256': h([f"{e['word']}:{e['score']}" for e in words]), 'head': words[:5]}

stems = practice.stem_words()
out['stems'] = {'count': len(stems), 'sha256': h(sorted(stems))}

# tokenizálás
checker = dictionary.get_checker()
sample = rng.sample(sorted(checker._entries), 3000) + ['KÉSZSÉG', 'ASSZONY', 'SZÉKSZÁM', 'GYÓGYÍTÓ', 'LLY', 'ZSZS', 'CSCS', 'TTY', 'NNY', 'SSZ']
lines = []
for w in sample:
    t = tokenize_word(w)
    lines.append(w + ':' + ('' if t is None else ','.join(t)) + ':' + str(word_base_score(w)))
out['tokenize'] = {'count': len(lines), 'items': [l.split(':') for l in lines]}

# racks
racks = []
for _ in range(25):
    racks.append(practice._draw_rack(rng))
racks.append(['S','Z','S','A','L','M','A'])
racks.append(['C','S','K','A','T','Z','S'])
rw = []
for rack in racks:
    words = practice.rack_words(rack)
    rw.append({'rack': rack, 'count': len(words), 'sha256': h([f"{w['word']}:{w['score']}:{w['tiles']}" for w in words]), 'top': words[:3]})
out['rack_words'] = rw

cases = []
for rack in racks[:12]:
    ws = practice.rack_words(rack)
    for w in ws[:3]:
        cases.append([rack, w['word']])
    cases.append([rack, 'XYZ'])
    cases.append([rack, 'A'])
    cases.append([rack, ''.join(rack)])
    cases.append([rack, ''.join(reversed(rack))])
cases += [[['S','Z','A','L','M','A','K'], 'SZALMA'], [['S','Z','A','L','M','A','K'], 'SZÁLMA'], [['S','Z','A','L','M','A','K'], 'ALMASZ']]
cases += [[['SZ','A','L','M','A','K','E'], 'SZALMA'], [['SZ','A','L','M','A','K','E'], 'ALMASZ'], [['Z','S','A','L','M','A','K'], 'ZSAK']]
chk = []
for rack, word in cases:
    chk.append({'rack': rack, 'word': word, 'result': practice.check_rack_word(rack, word)})
out['check_rack_word'] = chk

ans = []
for w in ['alma', 'almm', 'szék', 'SZÉKK', 'x', 'ABCDEFGHIJKLMNOP', 'quiz', 'ház', 'háaz', 'tarom', 'falim', 'kutya', 'kutyaa', 'zseb', ' zseb ', 'ÉJÉK']:
    for a in (True, False):
        ans.append({'word': w, 'answer': a, 'result': practice.check_answer(w, a)})
out['check_answer'] = ans
json.dump(out, open(os.path.join(OUT, 'practice.json'), 'w'), ensure_ascii=False)
print({k: (v['count'] if isinstance(v, dict) and 'count' in v else len(v)) for k, v in out.items()}, file=sys.stderr)

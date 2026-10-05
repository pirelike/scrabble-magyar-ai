import os, json, random, sys, time
sys.path.insert(0, os.environ['PYTHON_FINAL_DIR'])
OUT = os.environ.get('GOLDEN_OUT', 'tests/golden')
import dictionary, ai_player
from affix_checker import AffixChecker

t0 = time.time()
checker = dictionary.get_checker()
print('loaded', checker.entry_count, time.time() - t0, file=sys.stderr)
rng = random.Random(12345)

stems = sorted(checker._entries.keys())
sample_stems = rng.sample(stems, 2500)
words = set(sample_stems)

# ragozott alakok
adds = ai_player._INFLECT_ADDS
short = [s for s in stems if len(s) <= 6][:]
forms = sorted(checker.inflected_forms(rng.sample(short, 3000), adds, 12, risky=True))
words.update(rng.sample(forms, min(3000, len(forms))))
forms_nr = sorted(checker.inflected_forms(rng.sample(short, 3000), adds, 12, risky=False))
words.update(rng.sample(forms_nr, min(1500, len(forms_nr))))

# mutációk
letters = 'aábcdeéfghiíjklmnoóöőpqrstuúüűvwxyz'
for s in rng.sample(stems, 3000):
    if len(s) < 3: continue
    i = rng.randrange(len(s))
    kind = rng.random()
    if kind < 0.4: m = s[:i] + rng.choice(letters) + s[i+1:]
    elif kind < 0.7: m = s[:i] + s[i+1:]
    else: m = s[:i] + rng.choice(letters) + s[i:]
    words.add(m)
# két toldalék: szótő + véletlen végződések
for s in rng.sample(short, 1500):
    w = s + rng.choice(['ban','ben','ok','ek','nak','nek','ja','je','unk','ig','ra','ni','ás'])
    words.add(w)
    words.add(w + rng.choice(['ban','ek','ot','tól','ra','é']))
# előtagos
for s in rng.sample(stems, 800):
    words.add('meg' + s); words.add('legjobb' + s); words.add('újra' + s); words.add('el' + s + 'ik')
# kockázatos és használt alakok
attested = sorted(checker._attested)
words.update(rng.sample(attested, 1500))
words.update(['falim','tarom','éjék','blökiül','vu','kedvesem','drágám','királyné','almáné','feleségül','ajándékul',
              'szomszédék','anyámék','cirmosék','rosszul','véletlenül','tanulóm','fagyasztómból','gitárosok','dombosom',
              'kg','db','tv','brr','hm','pszt','dunát','budapest','duna','pagonyagy','kszi','mama','apa','szék','zseb'])
words = sorted(w for w in words if w and len(w) <= 15)

rows = []
for w in words:
    rows.append([w, int(checker.check(w)), int(checker.has_stem(w)), int(checker.needs_attestation(w)),
                 int(dictionary._lookup(w))])
out = {'words': rows}
json.dump(out, open(os.path.join(OUT, 'dictionary.json'), 'w'), ensure_ascii=False)
print('words', len(rows), 'valid', sum(r[1] for r in rows), 'attest', sum(r[3] for r in rows), file=sys.stderr)

# a teljes szókincs
t0 = time.time()
vocab = ai_player.load_vocabulary()
print('vocab', len(vocab), time.time() - t0, file=sys.stderr)
open(sys.argv[1], 'w').write('\n'.join(vocab.words))

# explain minta
ex = {}
for w in rng.sample(forms, 60) + ['kedvesem', 'tarom', 'királyné', 'szomszédék', 'feleségül', 'rosszul', 'házak', 'megházak']:
    ex[w] = checker.explain(w)
json.dump(ex, open(os.path.join(OUT, 'explain.json'), 'w'), ensure_ascii=False)

# javaslatok
sug = {}
for w in ['ALAMA', 'KUTYAA', 'SZÉKK', 'HÁZZ', 'MACSKÁ', 'TARTOM', 'XYZ', 'BÁRÁNY', 'ZSEBB', 'FÁLIM']:
    sug[w] = dictionary.suggest_words(w, 6)
json.dump(sug, open(os.path.join(OUT, 'suggest.json'), 'w'), ensure_ascii=False)

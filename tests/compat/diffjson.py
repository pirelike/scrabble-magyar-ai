import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import harness
from harness import leaf_diff
name = sys.argv[1]; skip = sys.argv[2] if len(sys.argv) > 2 else ''
b = harness.OUT
py = json.load(open(b + name + '.py.json')); rs = json.load(open(b + name + '.rs.json'))
n = 0
for a, c in zip(py, rs):
    if skip and a['label'].startswith(skip): continue
    d = [x for x in leaf_diff(a, c) if 'cookie_attrs' not in x and 'content-disposition' not in x]
    if d:
        n += 1
        print('##', a['label']); [print('   ' + x[:250]) for x in d[:8]]
print('összesen', n, 'eltérő kérés /', len(py))

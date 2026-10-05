import json, os, sys
t = json.load(open(os.environ.get('COMPAT_OUT', 'out') + '/%s.%s.json' % (sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else 'rs')))
for e in t:
    if 'note' in e:
        print('NOTE', e['step'], json.dumps(e['note'], ensure_ascii=False)[:200]); continue
    evs = []
    for l, ev in e['clients'].items():
        for x in ev:
            m = x[1].get('message') if isinstance(x[1], dict) else None
            evs.append(f"{l}:{x[0]}" + (f"({m[:50]})" if m else ''))
    print(e['step'], '|', ' '.join(evs)[:400])

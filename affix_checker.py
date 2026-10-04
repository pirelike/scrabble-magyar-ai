"""Beépített, külső függőség nélküli Hunspell-szerű szóellenőrző a dict/hu_HU.{aff,dic} fájlokhoz.

Miért kell? A pyenchant a rendszer libenchant könyvtárára, a hunspell CLI pedig a `hunspell`
programra épül, de egy egyszerű (pl. Render-féle Python) tárhelyen egyik sincs telepítve.
Ilyenkor korábban minden szót elfogadtunk, így a játék tetszőleges betűsort érvényesnek vett.
Ez a modul tisztán Pythonban végzi el ugyanazt: a szótő-szótárból (.dic) és a ragozási
szabályokból (.aff) eldönti, hogy egy szó alakja létezik-e.

Támogatott: szótő, egy előtag, legfeljebb két (egymásra épülő) végződés, a folytatási
osztályok (az `AF` aliasok), NEEDAFFIX / ONLYINCOMPOUND / FORBIDDENWORD jelzők.
Szándékosan NINCS összetételi (COMPOUND*) támogatás: a Hunspell összetétel-szabályai
tetszőleges betűsorokat is elfogadnak (pl. PAGONYAGY, KSZI), a Scrabble-ban viszont csak a
szótárban szereplő szavak és azok ragozott alakjai érvényesek.

A szavakat kisbetűvel kell keresni (a nagy kezdőbetűs tulajdonnevek így nem érvényesek).

A szótár (helyesírás-ellenőrzésre készült) minden nyelvtanilag lehetséges alakot elfogad, a Scrabble-ban
viszont ez sok értelmetlen szót engedne. Ezért a szótár morfológiai címkéi (`AM` aliasok: szófaj, a
toldalék fajtája) alapján:
- a csak kötőjellel toldalékolható szócikkek (rövidítések, mértékegységek, idegen írásmódú szavak:
  UV, COS, KCAL, MADAME) nem érvényesek;
- a "kockázatos" levezetések — melléknév (képzett melléknév, -s foglalkozásnév) birtokos személyjellel
  (KEDVESEM, de TAROM, FALIJA), -ék családi többes (SZOMSZÉDÉK, de ÉJÉK), -ul/-ül főnéven (FELESÉGÜL,
  de BLÖKIÜL) és a -né képző (KIRÁLYNÉ, de ALMÁNÉ) — csak akkor érvényesek, ha az alak a használatban
  ténylegesen előfordul (`attested_path`: a gyakorisági listából előre kiszűrt alakok, lásd
  `tools/build_attested.py`).
"""
import re

_WS_RE = re.compile(rb'[ \t]+')
_MAX_AFFIX_LEN = 40

# A szabályok kockázati jelzői (a morfológiai címkéjükből)
_RISK_FAMILIAR = 1      # -ék családi többes (is:ék_FAMILIAR_noun)
_RISK_ESSIVE = 2        # -ul/-ül (is:ESS): főnéven kockázatos (FELESÉGÜL, de BLÖKIÜL); melléknéven a
                        # szabályos határozószó-képzés (ROSSZUL, VÉLETLENÜL, ANGOLUL)
_RISK_POSS = 4          # birtokos személyjel / birtokjel (is:POSS..., is:POSSESSEE): melléknéven kockázatos
_RISK_DERIVED = 8       # ugyanabban a szabályban képző + az előbbiek kockázatos párosítása
_RISK_DERIVATION = 16   # -né képző (ORVOSNÉ, SÓGORNÉ, de ALMÁNÉ)
_RISK_ALWAYS = _RISK_FAMILIAR | _RISK_DERIVED | _RISK_DERIVATION
_RISK_NAMES = ((_RISK_FAMILIAR, 'familiar'), (_RISK_ESSIVE, 'essive'), (_RISK_POSS, 'possessive'),
               (_RISK_DERIVED, 'derived'), (_RISK_DERIVATION, 'derivation'))
_RISKY_DERIVATIONS = (b'ds:n\xc3\xa9_MRS_',)
# Képzők, amelyek után a szó birtokos személyjellel melléknévként kezelendő (kockázatos): a -s
# foglalkozásnév (GITÁROSOK rendben, de DOMBOSOM, BODZÁSUNK) — illetve főnévként (rendben): a
# folyamatos és a beálló melléknévi igenév (TANULÓM, FAGYASZTÓMBÓL, BEADANDÓIM)
_ADJ_LIKE_DERIVATIONS = (b'ds:s_OCCUPATION_',)
_NOUN_LIKE_DERIVATIONS = (b'ds:\xc3\x93_PRESPART_', b'ds:And\xc3\x93_FUTPART_')

# A szócikkek fajtája (a morfológiai leírásukból)
_KIND_ADJ = 1       # melléknév (po:adj)
_KIND_FOREIGN = 2   # csak kötőjellel toldalékolható főnév (al:szó-): rövidítés, mértékegység, idegen írásmód

_POS_ADJ_RE = re.compile(rb'(?:^|\s)po:adj(?:\s|$)')
_POS_NOUN_RE = re.compile(rb'(?:^|\s)po:noun(?:\s|$)')


def _split_ws(line):
    return _WS_RE.split(line.strip(b' \t\r'))


def _rule_risk(morph):
    """(kockázati jelzők, képzett szófaj: 'noun' | 'adj' | None) egy szabály morfológiai leírásából."""
    if not morph:
        return 0, None
    tokens = morph.split()
    derived = None
    for token in tokens:
        if token.startswith(_ADJ_LIKE_DERIVATIONS):
            derived = 'adj'
        elif token.startswith(_NOUN_LIKE_DERIVATIONS):
            derived = 'noun'
        elif token.startswith(b'ds:'):
            derived = 'noun' if token.endswith(b'_noun') else 'adj' if token.endswith(b'_adj') else 'other'
        elif token.startswith(b'is:') and token.endswith((b'_adj', b'_vrb')):
            # a szótár néhány képzőt is:-sel jelöl (-i, -bb, -bbik, -nyi, -jú, -hat)
            derived = 'adj' if token.endswith(b'_adj') else 'other'
    risk = 0
    if any(b'_FAMILIAR' in t for t in tokens if t.startswith(b'is:')):
        risk |= _RISK_FAMILIAR
    if b'is:ESS' in tokens:
        if derived == 'noun':
            risk |= _RISK_DERIVED
        elif derived is None:
            risk |= _RISK_ESSIVE
    if any(t.startswith(_RISKY_DERIVATIONS) for t in tokens):
        risk |= _RISK_DERIVATION
    if any(t.startswith(b'is:POSS') for t in tokens):
        if derived == 'adj':
            risk |= _RISK_DERIVED
        elif derived is None:
            risk |= _RISK_POSS
    return risk, derived


class _Rule:
    """Egy ragozási szabály (PFX vagy SFX sor)."""

    __slots__ = ('flag', 'cross', 'strip', 'add', 'cont', 'cond', 'need_affix', 'only_in_compound',
                 'risk', 'derived')

    def __init__(self, flag, cross, strip, add, cont, cond, need_affix, only_in_compound,
                 risk=0, derived=None):
        self.flag = flag
        self.cross = cross
        self.strip = strip
        self.add = add
        self.cont = cont  # bytes (folytatási jelzők) vagy None
        self.cond = cond  # lefordított reguláris kifejezés vagy None
        self.need_affix = need_affix
        self.only_in_compound = only_in_compound
        self.risk = risk          # _RISK_* jelzők
        self.derived = derived    # a képzett szófaj ('noun' / 'adj' / 'other'), ha a szabály képző


def _is_risky(kind, rule, outer=None):
    """Kockázatos-e a (`kind` fajtájú) szótő + `rule` (+ `outer`) levezetés?"""
    if rule.risk & _RISK_ALWAYS:
        return True
    adj = bool(kind & _KIND_ADJ)
    if rule.risk & _RISK_POSS and adj or rule.risk & _RISK_ESSIVE and not adj:
        return True
    if outer is not None:
        if outer.risk & _RISK_ALWAYS:
            return True
        # a belső szabály után a szófaj: a képzett, vagy (ha nem képző) a szótőé
        inner_adj = rule.derived == 'adj' or (rule.derived is None and adj)
        if outer.risk & _RISK_POSS and inner_adj or outer.risk & _RISK_ESSIVE and not inner_adj:
            return True
    return False


class AffixChecker:
    """Betöltés: AffixChecker(aff_útvonal, dic_útvonal). Keresés: `check(szó)` (kisbetűs szóval).

    attested_path: a ténylegesen használt "kockázatos" alakok listája (soronként egy szó, `#`
    megjegyzés); None esetén a kockázatos levezetések is érvényesek (a hunspell viselkedése).
    """

    def __init__(self, aff_path, dic_path, attested_path=None):
        self._aliases = [b'']  # 1-től indexelt
        self._morph_aliases = [b'']  # AM aliasok (1-től indexelt), csak a betöltés idejére
        self._needaffix = None
        self._onlyincompound = None
        self._forbidden = None
        self._pfx_cross = {}
        self._sfx_cross = {}
        self._prefixes = {}  # {add: [_Rule]}
        self._suffixes = {}  # {add: [_Rule]}
        self._cont_flags = set()  # azok a jelzők, amelyek valamelyik folytatási osztályban szerepelnek
        self._entries = {}  # {szó: [flag-bájtok, ...]}
        self._entry_kinds = {}  # {szó: (_KIND_* jelzők szócikkenként, az _entries sorrendjében)} — csak a nem 0-k
        self._sfx_by_flag = None  # lusta: {jelző: [végződés-szabály]}
        self._load_aff(aff_path)
        self._load_dic(dic_path)
        self._morph_aliases = None
        self._attested = self._load_attested(attested_path) if attested_path else None

    # ---------- betöltés ----------

    def _flag_of(self, token):
        return token[0] if token else None

    def _alias(self, token):
        """Folytatási / szótári jelzők: az `AF` alias sorszáma (vagy közvetlen jelzők, ha nincs alias)."""
        if not token:
            return None
        if self._aliases_defined and token.isdigit():
            index = int(token)
            if 0 < index < len(self._aliases):
                return self._aliases[index]
            return None
        return token

    def _load_aff(self, path):
        with open(path, 'rb') as f:
            lines = f.read().split(b'\n')

        self._aliases_defined = any(line.startswith(b'AF ') for line in lines)
        self._morph_aliases_defined = any(line.startswith(b'AM ') for line in lines)

        header_seen = False
        morph_header_seen = False
        rules = []
        for line in lines:
            if not line or line.startswith(b'#'):
                continue
            parts = _split_ws(line)
            key = parts[0]
            if key == b'AF':
                if not header_seen:
                    header_seen = True  # az első AF sor a darabszám
                    continue
                self._aliases.append(parts[1] if len(parts) > 1 else b'')
            elif key == b'AM':
                if not morph_header_seen:
                    morph_header_seen = True  # az első AM sor a darabszám
                    continue
                self._morph_aliases.append(line.strip(b' \t\r')[2:].strip())
            elif key == b'NEEDAFFIX' or (key == b'ONLYROOT' and self._needaffix is None):
                self._needaffix = self._flag_of(parts[1])
            elif key == b'ONLYINCOMPOUND':
                self._onlyincompound = self._flag_of(parts[1])
            elif key == b'FORBIDDENWORD':
                self._forbidden = self._flag_of(parts[1])
            elif key in (b'PFX', b'SFX'):
                rules.append((key, parts))

        for key, parts in rules:
            flag = self._flag_of(parts[1])
            if len(parts) == 4 and parts[2] in (b'Y', b'N') and parts[3].isdigit():
                (self._pfx_cross if key == b'PFX' else self._sfx_cross)[flag] = parts[2] == b'Y'
                continue
            if len(parts) < 4:
                continue
            self._add_rule(key == b'PFX', flag, parts)

    def _add_rule(self, is_prefix, flag, parts):
        strip = parts[2].decode('utf-8', 'replace')
        add_field, _, cont_field = parts[3].partition(b'/')
        add = add_field.decode('utf-8', 'replace')
        strip = '' if strip == '0' else strip
        add = '' if add == '0' else add
        cont = self._alias(cont_field) if cont_field else None
        condition = parts[4].decode('utf-8', 'replace') if len(parts) > 4 else '.'

        cond = None
        if condition != '.':
            try:
                cond = re.compile(('^' + condition) if is_prefix else (condition + '$'))
            except re.error:
                return

        cross = (self._pfx_cross if is_prefix else self._sfx_cross).get(flag, False)
        need_affix = bool(cont) and self._needaffix is not None and self._needaffix in cont
        only_in_compound = bool(cont) and self._onlyincompound is not None and self._onlyincompound in cont
        risk, derived = (0, None) if is_prefix else _rule_risk(self._morph(b' '.join(parts[5:])))
        rule = _Rule(flag, cross, strip, add, cont or None, cond, need_affix, only_in_compound,
                     risk, derived)

        (self._prefixes if is_prefix else self._suffixes).setdefault(add, []).append(rule)
        if cont:
            self._cont_flags.update(cont)

    def _morph(self, field):
        """Morfológiai leírás: az `AM` alias sorszáma (vagy maga a leírás, ha nincs alias)."""
        field = field.strip()
        if not field:
            return b''
        if self._morph_aliases_defined and field.isdigit():
            index = int(field)
            return self._morph_aliases[index] if 0 < index < len(self._morph_aliases) else b''
        return field

    def _entry_kind(self, word, morph):
        kind = 0
        if _POS_ADJ_RE.search(morph):
            kind |= _KIND_ADJ
        if b'al:' + word + b'-' in morph and _POS_NOUN_RE.search(morph):
            kind |= _KIND_FOREIGN
        return kind

    def _load_dic(self, path):
        entries = self._entries
        kinds = {}
        with open(path, 'rb') as f:
            first = True
            for raw in f:
                if first:
                    first = False
                    if raw.strip().isdigit():
                        continue
                line = raw.rstrip(b'\r\n')
                if not line:
                    continue
                entry, _, morph_field = line.partition(b'\t')
                word, _, flags = entry.partition(b'/')
                if b' ' in word:
                    continue  # többszavas tétel
                flags = flags.split(b' ', 1)[0]
                flag_set = self._alias(flags) if flags else b''
                if flag_set is None:
                    flag_set = b''
                key = word.decode('utf-8', 'replace')
                entry_list = entries.setdefault(key, [])
                kind = self._entry_kind(word, self._morph(morph_field)) if morph_field else 0
                if kind or key in kinds:
                    kinds.setdefault(key, [0] * len(entry_list)).append(kind)
                entry_list.append(flag_set)
        self._entry_kinds = {word: tuple(k) for word, k in kinds.items()}

    @staticmethod
    def _load_attested(path):
        attested = set()
        with open(path, encoding='utf-8') as f:
            for line in f:
                word = line.strip()
                if word and not word.startswith('#'):
                    attested.add(word.lower())
        return attested

    # ---------- keresés ----------

    def _usable_entries(self, word):
        """(jelzők, fajta) párok a szó szócikkeiből; a tiltott, csak összetételben álló és a csak
        kötőjellel toldalékolható (rövidítés, idegen írásmódú) szócikkek nélkül."""
        entries = self._entries.get(word)
        if not entries:
            return ()
        kinds = self._entry_kinds.get(word)
        usable = []
        for i, flags in enumerate(entries):
            kind = kinds[i] if kinds is not None else 0
            if kind & _KIND_FOREIGN:
                continue
            if self._forbidden is not None and self._forbidden in flags:
                continue
            if self._onlyincompound is not None and self._onlyincompound in flags:
                continue
            usable.append((flags, kind))
        return usable

    def _stem_ok(self, stem, rule, pfx=None, outer_flag=None, need_flag=None, outer_rule=None,
                 allow_risky=True):
        """Van-e olyan szótári tétel `stem` alakkal, amely a `rule` (és az esetleges `pfx`) szabályt viseli?
        `allow_risky` hamis: a kockázatos levezetés (lásd `_is_risky`) nem számít."""
        for flags, kind in self._usable_entries(stem):
            if rule.flag not in flags and not (pfx is not None and pfx.cont and rule.flag in pfx.cont):
                continue
            if pfx is not None and pfx.flag not in flags and not (rule.cont and pfx.flag in rule.cont):
                continue
            if outer_flag is not None and not (rule.cont and outer_flag in rule.cont):
                continue
            if need_flag is not None and need_flag not in flags and not (rule.cont and need_flag in rule.cont):
                continue
            if not allow_risky and _is_risky(kind, rule, outer_rule):
                continue
            return True
        return False

    @staticmethod
    def _cond_ok(rule, stem):
        return rule.cond is None or rule.cond.search(stem) is not None

    def _suffix_candidates(self, word):
        """Azok az SFX szabályok, amelyek `add` része a szó vége, és marad szótő."""
        found = []
        n = len(word)
        for k in range(0, min(n - 1, _MAX_AFFIX_LEN) + 1):
            add = word[n - k:] if k else ''
            rules = self._suffixes.get(add)
            if rules:
                found.append((add, rules))
        return found

    def _prefix_candidates(self, word):
        found = []
        n = len(word)
        for k in range(0, min(n - 1, _MAX_AFFIX_LEN) + 1):
            add = word[:k]
            rules = self._prefixes.get(add)
            if rules:
                found.append((add, rules))
        return found

    def _suffix_check(self, word, pfx=None, outer_flag=None, need_flag=None, outer_rule=None,
                      allow_risky=True):
        """Szótő + egy végződés (a `pfx` előtaggal keresztezve, ha meg van adva)."""
        for add, rules in self._suffix_candidates(word):
            base = word[:len(word) - len(add)]
            for rule in rules:
                if outer_flag is not None and not rule.cont:
                    continue
                if rule.only_in_compound:
                    continue
                if outer_flag is None and rule.need_affix and not (
                        pfx is not None and not pfx.need_affix):
                    continue
                if pfx is not None and not (rule.cross and pfx.cross):
                    continue
                stem = base + rule.strip
                if not self._cond_ok(rule, stem):
                    continue
                if self._stem_ok(stem, rule, pfx, outer_flag, need_flag, outer_rule, allow_risky):
                    return True
        return False

    def _suffix_check_two(self, word, pfx=None, need_flag=None, allow_risky=True):
        """Szótő + két egymásra épülő végződés."""
        for add, rules in self._suffix_candidates(word):
            base = word[:len(word) - len(add)]
            for rule in rules:
                if rule.flag not in self._cont_flags or rule.only_in_compound:
                    continue
                if pfx is not None and not (rule.cross and pfx.cross):
                    continue
                stem = base + rule.strip
                if not self._cond_ok(rule, stem):
                    continue
                inner_pfx = pfx if pfx is not None and not (rule.cont and pfx.flag in rule.cont) else None
                if self._suffix_check(stem, inner_pfx, rule.flag, need_flag, rule, allow_risky):
                    return True
        return False

    def _prefix_check(self, word, two_suffixes, allow_risky=True):
        for add, rules in self._prefix_candidates(word):
            rest = word[len(add):]
            for rule in rules:
                if rule.only_in_compound:
                    continue
                stem = rule.strip + rest
                if not self._cond_ok(rule, stem):
                    continue
                if not two_suffixes:
                    if any(rule.flag in flags for flags, _kind in self._usable_entries(stem)):
                        return True
                    if rule.cross and self._suffix_check(stem, rule, allow_risky=allow_risky):
                        return True
                elif rule.cross and self._suffix_check_two(stem, rule, allow_risky=allow_risky):
                    return True
        return False

    def _compute(self, word, allow_risky=True):
        entries = self._entries.get(word)
        if entries and self._forbidden is not None and any(self._forbidden in f for f in entries):
            return False
        if self.has_stem(word):
            return True
        if self._prefix_check(word, False, allow_risky):
            return True
        if self._suffix_check(word, allow_risky=allow_risky):
            return True
        if self._suffix_check_two(word, allow_risky=allow_risky):
            return True
        return self._prefix_check(word, True, allow_risky)

    def _risky_allowed(self, word):
        return self._attested is None or word in self._attested

    def check(self, word):
        """Igaz, ha a (kisbetűs) szó a szótárban szerepel vagy annak ragozott alakja. A kockázatos
        levezetés (melléknév + birtokos személyjel, -ék, -ul/-ül főnéven, -né) csak a ténylegesen használt
        alakoknál számít."""
        return self._compute(word, self._risky_allowed(word))

    def has_stem(self, word):
        """Igaz, ha a szó önmagában (toldalék nélkül) érvényes szótári tétel."""
        entries = self._entries.get(word)
        if not entries or (self._forbidden is not None and any(self._forbidden in f for f in entries)):
            return False
        return any(self._needaffix is None or self._needaffix not in flags
                   for flags, _kind in self._usable_entries(word))

    def needs_attestation(self, word):
        """Igaz, ha a szó csak kockázatos levezetéssel érvényes (a használati listán múlik)."""
        return not self._compute(word, False) and self._compute(word, True)

    # ---------- levezetések (a szó-vizsgáló eszköznek) ----------

    def _matching_kind(self, stem, rule, pfx=None, outer_flag=None, need_flag=None):
        """Mint a `_stem_ok`, de a szócikk fajtáját adja vissza (None, ha nincs találat)."""
        for flags, kind in self._usable_entries(stem):
            if rule.flag not in flags and not (pfx is not None and pfx.cont and rule.flag in pfx.cont):
                continue
            if pfx is not None and pfx.flag not in flags and not (rule.cont and pfx.flag in rule.cont):
                continue
            if outer_flag is not None and not (rule.cont and outer_flag in rule.cont):
                continue
            if need_flag is not None and need_flag not in flags and not (rule.cont and need_flag in rule.cont):
                continue
            return kind
        return None

    def _suffix_hits(self, word, pfx=None, outer_flag=None):
        """A `_suffix_check` találatai: (szabály, szótő, szócikk-fajta)."""
        for add, rules in self._suffix_candidates(word):
            base = word[:len(word) - len(add)]
            for rule in rules:
                if outer_flag is not None and not rule.cont:
                    continue
                if rule.only_in_compound:
                    continue
                if outer_flag is None and rule.need_affix and not (pfx is not None and not pfx.need_affix):
                    continue
                if pfx is not None and not (rule.cross and pfx.cross):
                    continue
                stem = base + rule.strip
                if not self._cond_ok(rule, stem):
                    continue
                kind = self._matching_kind(stem, rule, pfx, outer_flag)
                if kind is not None:
                    yield rule, stem, kind

    @staticmethod
    def _risk_names(rule):
        return [name for bit, name in _RISK_NAMES if rule is not None and rule.risk & bit]

    def explain(self, word, limit=12):
        """A (kisbetűs) szó levezetései a szótár szabályaiból: [{'stem', 'prefix', 'suffixes', 'risky', 'risk',
        'adjective', 'derived'}]. A tiltott / csak összetételben álló szócikkek nélkül. A kockázatos levezetések
        (`risky`) csak a használati listán szereplő alakoknál érvényesek. Nem a döntéshez való (azt a `check`
        adja), hanem a szó-vizsgáló eszköz magyarázatához."""
        found = []

        def add(stem, prefix, suffixes, rules, kind, outer=None):
            if len(found) >= limit:
                return
            rule = rules[0] if rules else None
            risky = bool(rule is not None and _is_risky(kind, rule, outer))
            risk = self._risk_names(rule) + (self._risk_names(outer) if outer is not None else [])
            entry = {'stem': stem, 'prefix': prefix, 'suffixes': suffixes, 'risky': risky,
                     'risk': sorted(set(risk)), 'adjective': bool(kind & _KIND_ADJ),
                     'derived': (rules[0].derived if rules and rules[0].derived else None)}
            if entry not in found:
                found.append(entry)

        if self.has_stem(word):
            add(word, None, [], [], 0)
        for add_text, prules in self._prefix_candidates(word):
            rest = word[len(add_text):]
            for prule in prules:
                if prule.only_in_compound:
                    continue
                stem = prule.strip + rest
                if not self._cond_ok(prule, stem):
                    continue
                for flags, kind in self._usable_entries(stem):
                    if prule.flag in flags:
                        add(stem, add_text, [], [prule], kind)
                        break
                if prule.cross:
                    for srule, sstem, kind in self._suffix_hits(stem, prule):
                        add(sstem, add_text, [srule.add], [srule], kind)
        for srule, stem, kind in self._suffix_hits(word):
            add(stem, None, [srule.add], [srule], kind)
        for add_text, orules in self._suffix_candidates(word):
            base = word[:len(word) - len(add_text)]
            for orule in orules:
                if orule.flag not in self._cont_flags or orule.only_in_compound:
                    continue
                inner = base + orule.strip
                if not self._cond_ok(orule, inner):
                    continue
                for irule, stem, kind in self._suffix_hits(inner, None, orule.flag):
                    add(stem, None, [irule.add, orule.add], [irule], kind, orule)
        return found

    # ---------- ragozott alakok előállítása ----------

    def _rules_by_flag(self):
        """A végződés-szabályok jelzők szerint csoportosítva (egyszer épül fel)."""
        if self._sfx_by_flag is None:
            by_flag = {}
            for rules in self._suffixes.values():
                for rule in rules:
                    by_flag.setdefault(rule.flag, []).append(rule)
            self._sfx_by_flag = by_flag
        return self._sfx_by_flag

    def inflected_forms(self, stems, adds, max_form_len=15, risky=True):
        """A megadott szótövek ragozott alakjai: szótő + EGY végződés, a szótár saját szabályaival.

        stems: kisbetűs szótövek (a .dic szócikkei); adds: a megengedett végződések (pl. {'ban',
        'ek'}) — a teljes szabályrendszer több tízmillió alakot adna, ezért csak a kért végződésekkel
        dolgozunk. A tővégi a/e nyúlásával járó alak (alma → almá-ban: a szabály `add` része 'ában')
        ugyanannak a végződésnek számít. A szótári szavakat (és a szótárban magában szereplő alakokat)
        nem adja vissza újra. A kockázatos levezetésből (lásd `check`) csak a ténylegesen használt
        alakokat adja — `risky` hamis értékénél egyet sem. Visszatér: a ragozott alakok halmaza.
        """
        by_flag = self._rules_by_flag()

        def wanted_rule(rule):
            if rule.need_affix or rule.only_in_compound:
                return False
            if rule.add in adds:
                return True
            return rule.strip in ('a', 'e') and rule.add[:1] in ('á', 'é') and rule.add[1:] in adds

        wanted = {flag: [r for r in rules if wanted_rule(r)] for flag, rules in by_flag.items()}
        forms = set()
        for stem in stems:
            for flags, kind in self._usable_entries(stem):
                for flag in flags:
                    for rule in wanted.get(flag, ()):
                        if rule.strip and not stem.endswith(rule.strip):
                            continue
                        if rule.cond is not None and rule.cond.search(stem) is None:
                            continue
                        form = (stem[:len(stem) - len(rule.strip)] if rule.strip else stem) + rule.add
                        if form == stem or len(form) > max_form_len or form in self._entries:
                            continue
                        if _is_risky(kind, rule) and not (risky and self._risky_allowed(form)):
                            continue
                        forms.add(form)
        return forms

    def __contains__(self, word):
        return self.check(word)

    @property
    def entry_count(self):
        return len(self._entries)

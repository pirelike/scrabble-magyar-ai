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
"""
import re

_WS_RE = re.compile(rb'[ \t]+')
_MAX_AFFIX_LEN = 40


def _split_ws(line):
    return _WS_RE.split(line.strip(b' \t\r'))


class _Rule:
    """Egy ragozási szabály (PFX vagy SFX sor)."""

    __slots__ = ('flag', 'cross', 'strip', 'add', 'cont', 'cond', 'need_affix', 'only_in_compound')

    def __init__(self, flag, cross, strip, add, cont, cond, need_affix, only_in_compound):
        self.flag = flag
        self.cross = cross
        self.strip = strip
        self.add = add
        self.cont = cont  # bytes (folytatási jelzők) vagy None
        self.cond = cond  # lefordított reguláris kifejezés vagy None
        self.need_affix = need_affix
        self.only_in_compound = only_in_compound


class AffixChecker:
    """Betöltés: AffixChecker(aff_útvonal, dic_útvonal). Keresés: `check(szó)` (kisbetűs szóval)."""

    def __init__(self, aff_path, dic_path):
        self._aliases = [b'']  # 1-től indexelt
        self._needaffix = None
        self._onlyincompound = None
        self._forbidden = None
        self._pfx_cross = {}
        self._sfx_cross = {}
        self._prefixes = {}  # {add: [_Rule]}
        self._suffixes = {}  # {add: [_Rule]}
        self._cont_flags = set()  # azok a jelzők, amelyek valamelyik folytatási osztályban szerepelnek
        self._entries = {}  # {szó: [flag-bájtok, ...]}
        self._load_aff(aff_path)
        self._load_dic(dic_path)

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

        header_seen = False
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
        rule = _Rule(flag, cross, strip, add, cont or None, cond, need_affix, only_in_compound)

        (self._prefixes if is_prefix else self._suffixes).setdefault(add, []).append(rule)
        if cont:
            self._cont_flags.update(cont)

    def _load_dic(self, path):
        entries = self._entries
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
                entry = line.split(b'\t', 1)[0]
                word, _, flags = entry.partition(b'/')
                if b' ' in word:
                    continue  # többszavas tétel
                flags = flags.split(b' ', 1)[0]
                flag_set = self._alias(flags) if flags else b''
                if flag_set is None:
                    flag_set = b''
                entries.setdefault(word.decode('utf-8', 'replace'), []).append(flag_set)

    # ---------- keresés ----------

    def _stem_ok(self, stem, rule, pfx=None, outer_flag=None, need_flag=None):
        """Van-e olyan szótári tétel `stem` alakkal, amely a `rule` (és az esetleges `pfx`) szabályt viseli?"""
        for flags in self._entries.get(stem, ()):
            if self._forbidden is not None and self._forbidden in flags:
                continue
            if self._onlyincompound is not None and self._onlyincompound in flags:
                continue
            if rule.flag not in flags and not (pfx is not None and pfx.cont and rule.flag in pfx.cont):
                continue
            if pfx is not None and pfx.flag not in flags and not (rule.cont and pfx.flag in rule.cont):
                continue
            if outer_flag is not None and not (rule.cont and outer_flag in rule.cont):
                continue
            if need_flag is not None and need_flag not in flags and not (rule.cont and need_flag in rule.cont):
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

    def _suffix_check(self, word, pfx=None, outer_flag=None, need_flag=None):
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
                if self._stem_ok(stem, rule, pfx, outer_flag, need_flag):
                    return True
        return False

    def _suffix_check_two(self, word, pfx=None, need_flag=None):
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
                if pfx is not None and not (rule.cont and pfx.flag in rule.cont):
                    inner = self._suffix_check(stem, pfx, rule.flag, need_flag)
                else:
                    inner = self._suffix_check(stem, None, rule.flag, need_flag)
                if inner:
                    return True
        return False

    def _prefix_check(self, word, two_suffixes):
        for add, rules in self._prefix_candidates(word):
            rest = word[len(add):]
            for rule in rules:
                if rule.only_in_compound:
                    continue
                stem = rule.strip + rest
                if not self._cond_ok(rule, stem):
                    continue
                if not two_suffixes:
                    for flags in self._entries.get(stem, ()):
                        if self._forbidden is not None and self._forbidden in flags:
                            continue
                        if self._onlyincompound is not None and self._onlyincompound in flags:
                            continue
                        if rule.flag in flags:
                            return True
                    if rule.cross and self._suffix_check(stem, rule):
                        return True
                elif rule.cross and self._suffix_check_two(stem, rule):
                    return True
        return False

    def _compute(self, word):
        entries = self._entries.get(word)
        if entries:
            if self._forbidden is not None and any(self._forbidden in f for f in entries):
                return False
            for flags in entries:
                if self._needaffix is not None and self._needaffix in flags:
                    continue
                if self._onlyincompound is not None and self._onlyincompound in flags:
                    continue
                return True
        if self._prefix_check(word, False):
            return True
        if self._suffix_check(word):
            return True
        if self._suffix_check_two(word):
            return True
        return self._prefix_check(word, True)

    def check(self, word):
        """Igaz, ha a (kisbetűs) szó a szótárban szerepel vagy annak ragozott alakja."""
        return self._compute(word)

    def __contains__(self, word):
        return self.check(word)

    @property
    def entry_count(self):
        return len(self._entries)

"""affix_checker.py: a beágyazott, Hunspell-szerű szóellenőrző (kis, kézzel írt szótárakkal)."""
import os
import random
import re
import shutil
import subprocess

import pytest

from affix_checker import AffixChecker


def make_checker(tmp_path, aff, dic):
    aff_path = tmp_path / 'test.aff'
    dic_path = tmp_path / 'test.dic'
    aff_path.write_bytes(aff if isinstance(aff, bytes) else aff.encode('utf-8'))
    dic_path.write_bytes(dic if isinstance(dic, bytes) else dic.encode('utf-8'))
    return AffixChecker(str(aff_path), str(dic_path))


class TestStems:
    def test_stem_lookup(self, tmp_path):
        c = make_checker(tmp_path, 'SET UTF-8\n', '2\nalma\nkörte\n')
        assert c.check('alma') and c.check('körte')
        assert not c.check('alm') and not c.check('almák')

    def test_dic_extras_are_ignored(self, tmp_path):
        dic = '4\nkutya\t12\nmacska/\nad hoc\n\nBudapest\n'
        c = make_checker(tmp_path, 'SET UTF-8\n', dic)
        assert c.check('kutya') and c.check('macska')
        assert not c.check('ad hoc')      # többszavas tétel
        assert not c.check('budapest')    # a tulajdonnév csak nagy kezdőbetűvel van meg
        assert c.entry_count == 3

    def test_entries_without_count_header(self, tmp_path):
        c = make_checker(tmp_path, 'SET UTF-8\n', 'alma\nkörte\n')
        assert c.check('alma') and c.check('körte')


class TestSuffixes:
    AFF = (
        'SET UTF-8\n'
        'SFX A Y 3\n'
        'SFX A 0 t .\n'              # alma -> almat (egyszerű toldalék)
        'SFX A a át a\n'             # alma -> almát (levágás: a -> át)
        'SFX A 0 ek [^aáeé]\n'       # feltétel: csak mássalhangzós tő után
    )

    def test_plain_suffix(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nalma/A\n')
        assert c.check('almat')

    def test_strip_and_condition(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '2\nalma/A\nkönyv/A\n')
        assert c.check('almát')
        assert c.check('könyvek')
        assert not c.check('almaek')       # a feltétel (nem magánhangzós tő) nem teljesül
        assert not c.check('könyvát')      # a levágandó "a" nincs a tő végén

    def test_flag_is_required(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '2\nalma/A\nkörte\n')
        assert not c.check('körtet')       # a körtének nincs A jelzője

    def test_a_suffix_cannot_stand_alone(self, tmp_path):
        aff = 'SET UTF-8\nSFX A Y 1\nSFX A 0 ek .\n'
        assert make_checker(tmp_path, aff, '1\nek/A\n').check('ekek')   # tő: "ek", toldalék: "ek"
        assert not make_checker(tmp_path, aff, '1\nalma/A\n').check('ek')


class TestPrefixes:
    AFF = (
        'SET UTF-8\n'
        'PFX P Y 1\nPFX P 0 meg .\n'
        'PFX Q N 1\nPFX Q 0 el .\n'
        'SFX S Y 1\nSFX S 0 ek .\n'
        'SFX T N 1\nSFX T 0 em .\n'
    )

    def test_prefix_only(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nír/P\n')
        assert c.check('megír') and not c.check('elír')

    def test_cross_product(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nír/PS\n')
        assert c.check('megírek')          # előtag + toldalék (mindkettő keresztezhető)

    def test_no_cross_product_if_prefix_or_suffix_forbids_it(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nír/QS\n')
        assert c.check('elír') and c.check('írek')
        assert not c.check('elírek')       # Q előtag N (nem keresztezhető)
        c = make_checker(tmp_path, self.AFF, '1\nír/PT\n')
        assert c.check('írem') and c.check('megír')
        assert not c.check('megírem')      # T toldalék N


class TestContinuationClasses:
    """Egymásra épülő toldalékok és az AF aliasok."""

    AFF = (
        'SET UTF-8\n'
        'AF 4\n'
        'AF B # 1\n'        # 1. alias: csak a B jelző
        'AF AB # 2\n'       # 2. alias: A és B jelző
        'AF u # 3\n'        # 3. alias: NEEDAFFIX jelző
        'AF uB # 4\n'       # 4. alias: NEEDAFFIX és B jelző
        'NEEDAFFIX u\n'
        'SFX A Y 1\nSFX A 0 ek/1 .\n'     # az "ek" után jöhet B
        'SFX B Y 1\nSFX B 0 ben .\n'
    )

    def test_two_suffixes(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nház/2\n')
        assert c.check('házek') and c.check('házben')
        assert c.check('házekben')

    def test_continuation_must_allow_the_outer_suffix(self, tmp_path):
        aff = self.AFF.replace('SFX A 0 ek/1 .', 'SFX A 0 ek .')   # nincs folytatási osztály
        c = make_checker(tmp_path, aff, '1\nház/2\n')
        assert c.check('házek')
        assert not c.check('házekben')

    def test_outer_suffix_needs_its_own_flag_on_the_inner_one_only(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nház/B\n')
        assert c.check('házben')
        assert not c.check('házekben')     # a háznak nincs A jelzője, így "ek" nem kerülhet rá

    def test_stem_with_needaffix_is_not_a_word_alone(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '1\nház/4\n')
        assert not c.check('ház')
        assert c.check('házben')

    def test_aliases_are_shared_between_entries(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '2\nház/2\nkert/2\n')
        assert c.check('kertekben') and c.check('házekben')

    def test_without_aliases_flags_are_literal(self, tmp_path):
        aff = 'SET UTF-8\nSFX A Y 1\nSFX A 0 ek .\nSFX B Y 1\nSFX B 0 ben .\n'
        c = make_checker(tmp_path, aff, '1\nház/AB\n')
        assert c.check('házek') and c.check('házben')
        assert not c.check('házekben')     # folytatási osztály nélkül nincs kettős toldalék

    def test_suffix_with_needaffix_continuation_is_not_final(self, tmp_path):
        aff = ('SET UTF-8\nAF 2\nAF Bu # 1\nAF A # 2\nNEEDAFFIX u\n'
               'SFX A Y 1\nSFX A 0 ek/1 .\n'
               'SFX B Y 1\nSFX B 0 ben .\n')
        c = make_checker(tmp_path, aff, '1\nház/2\n')
        assert not c.check('házek')        # az "ek" toldalék csak köztes lehet
        assert c.check('házekben')


class TestSpecialFlags:
    AFF = (
        'SET UTF-8\n'
        'FORBIDDENWORD w\n'
        'ONLYINCOMPOUND |\n'
        'SFX A Y 1\nSFX A 0 ak .\n'
    )

    def test_forbidden_words(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '2\nalma/Aw\nalma\n')
        assert not c.check('alma')         # a tiltott jelzős tétel mindent felülír

    def test_only_in_compound_stem(self, tmp_path):
        c = make_checker(tmp_path, self.AFF, '2\nfa/|A\nház/A\n')
        assert not c.check('fa') and not c.check('faak')
        assert c.check('ház') and c.check('házak')

    def test_homonym_entries(self, tmp_path):
        c = make_checker(tmp_path, 'SET UTF-8\nNEEDAFFIX u\nSFX A Y 1\nSFX A 0 ak .\n', '2\nlúd/uA\nlúd\n')
        assert c.check('lúd') and c.check('lúdak')


class TestBinaryFlags:
    """A hu_HU.aff egybájtos, nem UTF-8 jelzőket használ (és Latin-1 megjegyzéseket)."""

    def test_single_byte_flags_and_latin1_comments(self, tmp_path):
        aff = (b'# L\xe1szl\xf3 N\xe9meth\n'
               b'SET UTF-8\n'
               b'SFX \xff Y 1\n'
               b'SFX \xff 0 \xc3\xa1k .\n')
        dic = b'1\nalma/\xff\n'
        c = make_checker(tmp_path, aff, dic)
        assert c.check('almaák') and c.check('alma')

    def test_unparsable_condition_skips_only_that_rule(self, tmp_path):
        aff = 'SET UTF-8\nSFX A Y 2\nSFX A 0 ak [\nSFX A 0 ok .\n'
        c = make_checker(tmp_path, aff, '1\nház/A\n')
        assert c.check('házok') and not c.check('házak')


@pytest.fixture(scope='module')
def checker():
    import dictionary
    return AffixChecker(os.path.join(dictionary._DICT_DIR, 'hu_HU.aff'),
                        os.path.join(dictionary._DICT_DIR, 'hu_HU.dic'))


class TestRealDictionary:
    def test_loads_a_large_dictionary_quickly(self, checker):
        assert checker.entry_count > 80_000

    @pytest.mark.parametrize('word', ['alma', 'almát', 'almákat', 'körtéknek', 'legszebb', 'elmentek',
                                      'szavazás', 'szavazásokban', 'csaholsz'])
    def test_known_forms(self, checker, word):
        assert checker.check(word)

    @pytest.mark.parametrize('word', ['salyt', 'xyz', 'almaa', 'körtét' + 'k', 'qwertz', 'hhh'])
    def test_unknown_forms(self, checker, word):
        assert not checker.check(word)

    @pytest.mark.skipif(shutil.which('hunspell') is None, reason='hunspell nincs telepítve')
    def test_stems_and_typos_do_not_pass_where_hunspell_says_no(self, checker):
        """Tőszavakon és elgépelt alakjaikon a beépített ellenőrző nem lehet engedékenyebb a hunspellnél."""
        letters = re.compile(r'^[a-záéíóöőúüű]+$')
        stems = sorted(w for w in checker._entries if letters.match(w))
        rng = random.Random(11)
        sample = rng.sample(stems, 1500)
        typos = []
        for word in sample:
            i = rng.randrange(len(word))
            typos.append(word[:i] + rng.choice('aeiotnslkrzmáéö') + word[i + 1:])
        words = sorted(set(sample + typos))

        dict_dir = os.path.dirname(os.path.abspath(__file__)) + '/../dict'
        result = subprocess.run(['hunspell', '-d', dict_dir + '/hu_HU', '-l'], input='\n'.join(words),
                                capture_output=True, text=True, timeout=60,
                                env={**os.environ, 'DICPATH': dict_dir})
        hunspell_invalid = {w.strip() for w in result.stdout.split('\n') if w.strip()}
        too_lenient = [w for w in words if checker.check(w) and w in hunspell_invalid]
        assert too_lenient == []

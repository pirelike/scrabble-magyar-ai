"""Tests for challenge.py - Challenge state machine."""
import pytest
from unittest.mock import MagicMock


def _make_challenge(**kwargs):
    from challenge import Challenge
    defaults = dict(
        tiles_placed=[(7, 6, 'A', False), (7, 7, 'B', False)],
        formed_words=[('AB', [(7, 6), (7, 7)], 4)],
        word_strs=['AB'],
        score=4,
        player_idx=0,
        removed_from_hand=['A', 'B'],
    )
    defaults.update(kwargs)
    return Challenge(**defaults)


def _make_player(pid, name):
    p = MagicMock()
    p.id = pid
    p.name = name
    return p


class TestChallengeInit:
    def test_initial_state(self):
        c = _make_challenge()
        assert c.tiles_placed == [(7, 6, 'A', False), (7, 7, 'B', False)]
        assert c.word_strs == ['AB']
        assert c.score == 4
        assert c.player_idx == 0
        assert c.removed_from_hand == ['A', 'B']
        assert c.accepted_players == set()
        assert c.voting_phase is False
        assert c.challenger_id is None
        assert c.votes == {}


class TestStartVoting:
    def test_start_voting(self):
        c = _make_challenge()
        c.start_voting('p2')
        assert c.voting_phase is True
        assert c.challenger_id == 'p2'
        assert c.votes == {}

    def test_start_voting_carries_over_accepts(self):
        c = _make_challenge()
        c.add_accept('p3')
        c.add_accept('p4')
        c.start_voting('p2')
        assert c.voting_phase is True
        assert c.votes == {'p3': 'accept', 'p4': 'accept'}


class TestAddAccept:
    def test_add_accept(self):
        c = _make_challenge()
        c.add_accept('p2')
        assert 'p2' in c.accepted_players

    def test_add_accept_idempotent(self):
        c = _make_challenge()
        c.add_accept('p2')
        c.add_accept('p2')
        assert len(c.accepted_players) == 1


class TestAddVote:
    def test_add_vote_accept(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        assert c.votes['p3'] == 'accept'

    def test_add_vote_reject(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'reject')
        assert c.votes['p3'] == 'reject'


class TestAllAccepted:
    def test_all_accepted_true(self):
        c = _make_challenge()
        c.add_accept('p2')
        c.add_accept('p3')
        assert c.all_accepted({'p2', 'p3'}) is True

    def test_all_accepted_false(self):
        c = _make_challenge()
        c.add_accept('p2')
        assert c.all_accepted({'p2', 'p3'}) is False

    def test_all_accepted_empty(self):
        c = _make_challenge()
        assert c.all_accepted(set()) is True


class TestAllVoted:
    def test_all_voted_true(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        c.add_vote('p4', 'reject')
        assert c.all_voted({'p3', 'p4'}) is True

    def test_all_voted_false(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        assert c.all_voted({'p3', 'p4'}) is False

    def test_all_voted_empty(self):
        c = _make_challenge()
        c.start_voting('p2')
        assert c.all_voted(set()) is True


class TestResolveVotes:
    def test_no_voters_accepted(self):
        c = _make_challenge()
        c.start_voting('p2')
        assert c.resolve_votes(set()) == 'vote_accepted'

    def test_all_accept(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        c.add_vote('p4', 'accept')
        assert c.resolve_votes({'p3', 'p4'}) == 'vote_accepted'

    def test_all_reject(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'reject')
        c.add_vote('p4', 'reject')
        assert c.resolve_votes({'p3', 'p4'}) == 'vote_rejected'

    def test_50_percent_accepted(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        c.add_vote('p4', 'reject')
        assert c.resolve_votes({'p3', 'p4'}) == 'vote_accepted'

    def test_less_than_50_percent_rejected(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'reject')
        c.add_vote('p4', 'reject')
        c.add_vote('p5', 'accept')
        assert c.resolve_votes({'p3', 'p4', 'p5'}) == 'vote_rejected'

    def test_non_voters_count_as_accept(self):
        """Voters who haven't voted aren't in self.votes, treated as accept by resolve."""
        c = _make_challenge()
        c.start_voting('p2')
        # p3 didn't vote — resolve_votes checks votes.get(vid) != 'reject'
        assert c.resolve_votes({'p3'}) == 'vote_accepted'


class TestUpdatePlayerSid:
    def test_update_challenger_id(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.update_player_sid('p2', 'p2_new')
        assert c.challenger_id == 'p2_new'

    def test_update_accepted_player(self):
        c = _make_challenge()
        c.add_accept('p3')
        c.update_player_sid('p3', 'p3_new')
        assert 'p3_new' in c.accepted_players
        assert 'p3' not in c.accepted_players

    def test_update_voted_player(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        c.update_player_sid('p3', 'p3_new')
        assert c.votes.get('p3_new') == 'accept'
        assert 'p3' not in c.votes

    def test_update_nonexistent_player(self):
        c = _make_challenge()
        c.start_voting('p2')
        # Should not raise
        c.update_player_sid('p_unknown', 'p_new')
        assert c.challenger_id == 'p2'


class TestToStateDict:
    def test_basic_state(self):
        c = _make_challenge()
        players = [_make_player('p1', 'Alice'), _make_player('p2', 'Bob')]
        d = c.to_state_dict(players)
        assert d['player_id'] == 'p1'
        assert d['player_name'] == 'Alice'
        assert d['words'] == ['AB']
        assert d['score'] == 4
        assert len(d['tiles']) == 2
        assert d['voting_phase'] is False
        assert d['player_count'] == 2
        assert d['challenger_id'] is None

    def test_voting_state(self):
        c = _make_challenge()
        c.start_voting('p2')
        c.add_vote('p3', 'accept')
        players = [
            _make_player('p1', 'Alice'),
            _make_player('p2', 'Bob'),
            _make_player('p3', 'Charlie'),
        ]
        d = c.to_state_dict(players)
        assert d['voting_phase'] is True
        assert d['challenger_id'] == 'p2'
        assert d['challenger_name'] == 'Bob'
        assert d['votes'] == {'p3': 'accept'}
        assert d['player_count'] == 3

    def test_tiles_format(self):
        c = _make_challenge(
            tiles_placed=[(3, 5, 'SZ', True)],
        )
        players = [_make_player('p1', 'Alice')]
        d = c.to_state_dict(players)
        assert d['tiles'] == [{'row': 3, 'col': 5, 'letter': 'SZ', 'is_blank': True}]

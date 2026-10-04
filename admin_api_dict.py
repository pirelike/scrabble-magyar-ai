"""Admin API: szótár és moderáció (`/api/admin/dictionary...`, `/reports`, `/moderation...`)."""
from flask import g, make_response, request

import admin_dict
import admin_live
import admin_mod
from admin_routes import (
    admin_bp, body_or_empty, csv_response, danger, json_ok, reason_of, sudo_required, wants_csv,
)

# ===== Szótár =====


@admin_bp.route('/dictionary/word', methods=['GET'])
def dictionary_word():
    """Szó-vizsgáló: a teljes döntési lánc egy szóra."""
    return json_ok(**admin_dict.inspect_word(request.args.get('w')))


@admin_bp.route('/dictionary/rejected', methods=['GET'])
def dictionary_rejected_list():
    data = admin_dict.list_rejected(request.args)
    if wants_csv():
        return csv_response('admin-rejected.csv', ('word', 'listed', 'voted', 'allowed', 'good', 'bad'), data['items'])
    return json_ok(**data)


@admin_bp.route('/dictionary/rejected/preview', methods=['POST'])
def dictionary_rejected_preview():
    """Beillesztett lista előnézete: melyik szó vehető fel, melyik már a listán van, melyik nem is érvényes most."""
    return json_ok(**admin_dict.preview_add(body_or_empty().get('words')))


@admin_bp.route('/dictionary/rejected', methods=['POST'])
@danger
def dictionary_rejected_add():
    body = body_or_empty()
    result = admin_dict.add_rejected(g.admin_ctx, body.get('words'), reason_of(body))
    return json_ok(**result)


@admin_bp.route('/dictionary/rejected', methods=['DELETE'])
@danger
def dictionary_rejected_remove():
    body = body_or_empty()
    removed = admin_dict.remove_rejected(g.admin_ctx, body.get('words'), reason_of(body))
    return json_ok(removed=removed)


@admin_bp.route('/dictionary/rejected/download', methods=['GET'])
def dictionary_rejected_download():
    """A tartós lista fájlja (hu_rejected.txt), vagy a változások diffje (`diff=1`) a repóba való átvezetéshez."""
    if request.args.get('diff') == '1':
        text, name = admin_dict.list_diff(), 'hu_rejected.diff'
    else:
        text, name = admin_dict._read_list_text(), 'hu_rejected.txt'
    response = make_response(text)
    response.headers['Content-Type'] = 'text/plain; charset=utf-8'
    response.headers['Content-Disposition'] = f'attachment; filename="{name}"'
    return response


@admin_bp.route('/dictionary/rejected/export-votes', methods=['POST'])
@sudo_required
def dictionary_export_votes():
    """A szavazatokból kizárt szavak átvezetése a tartós listába."""
    return json_ok(**admin_dict.export_votes(g.admin_ctx, reason_of(body_or_empty())))


@admin_bp.route('/dictionary/overrides', methods=['GET'])
def dictionary_overrides_list():
    return json_ok(items=admin_dict.list_overrides())


@admin_bp.route('/dictionary/overrides', methods=['POST'])
@danger
def dictionary_overrides_set():
    body = body_or_empty()
    word = admin_dict.set_override(g.admin_ctx, body.get('word'), body.get('verdict'), reason_of(body))
    return json_ok(word=word)


@admin_bp.route('/dictionary/overrides', methods=['DELETE'])
@danger
def dictionary_overrides_remove():
    body = body_or_empty()
    admin_dict.remove_override(g.admin_ctx, body.get('word'), reason_of(body))
    return json_ok()


@admin_bp.route('/dictionary/additions', methods=['GET'])
def dictionary_additions_list():
    return json_ok(items=admin_dict.list_additions())


@admin_bp.route('/dictionary/additions', methods=['POST'])
@danger
def dictionary_additions_add():
    body = body_or_empty()
    return json_ok(word=admin_dict.add_addition(g.admin_ctx, body.get('word'), reason_of(body)))


@admin_bp.route('/dictionary/additions', methods=['DELETE'])
@danger
def dictionary_additions_remove():
    body = body_or_empty()
    admin_dict.remove_addition(g.admin_ctx, body.get('word'), reason_of(body))
    return json_ok()


@admin_bp.route('/dictionary/votes', methods=['DELETE'])
@sudo_required
def dictionary_votes_delete():
    body = body_or_empty()
    return json_ok(deleted=admin_dict.delete_votes(g.admin_ctx, body.get('word'), reason_of(body)))


@admin_bp.route('/dictionary/second-opinion', methods=['GET'])
def dictionary_second_opinion():
    return json_ok(items=admin_dict.second_opinion(request.args.get('n', admin_dict.SECOND_OPINION_DEFAULT,
                                                                     type=int)))


@admin_bp.route('/dictionary/reviewers', methods=['GET'])
def dictionary_reviewers():
    return json_ok(**admin_dict.reviewers())


@admin_bp.route('/dictionary/review-summary', methods=['GET'])
def dictionary_review_summary():
    return json_ok(**admin_dict.review_summary(request.args.get('days', 14, type=int)))


@admin_bp.route('/dictionary/threshold', methods=['GET'])
def dictionary_threshold():
    return json_ok(**admin_dict.threshold_info())


@admin_bp.route('/dictionary/caches/clear', methods=['POST'])
@danger
def dictionary_caches_clear():
    body = body_or_empty()
    return json_ok(**admin_dict.clear_cache(g.admin_ctx, body.get('which'), reason_of(body)))


# ===== Moderáció =====

@admin_bp.route('/reports', methods=['GET'])
def reports_list():
    return json_ok(**admin_mod.list_reports(request.args))


@admin_bp.route('/reports/<int:report_id>', methods=['GET'])
def reports_detail(report_id):
    return json_ok(report=admin_mod.get_report(g.admin_ctx, report_id))


@admin_bp.route('/reports/<int:report_id>', methods=['PATCH'])
def reports_update(report_id):
    body = body_or_empty()
    admin_mod.update_report(g.admin_ctx, report_id, body.get('status'), reason_of(body))
    return json_ok()


@admin_bp.route('/moderation/chat', methods=['GET'])
def moderation_chat():
    """A chat napló: a futó szobák üzenetei élőben (`source=live`, alapértelmezett), vagy a tartósan naplózottak
    (`source=log`). Naplózva: `view.chat`."""
    if request.args.get('source') == 'log':
        data = admin_mod.query_chat_log(g.admin_ctx, request.args)
        if wants_csv():
            return csv_response('admin-chat.csv', ('id', 'created_at', 'room_name', 'name', 'user_id', 'message'),
                                data['items'])
        return json_ok(**data)
    return json_ok(**admin_live.live_chat(g.admin_ctx, request.args))


@admin_bp.route('/moderation/words', methods=['GET'])
def moderation_words_list():
    return json_ok(items=admin_mod.list_banned_words())


@admin_bp.route('/moderation/words', methods=['POST'])
@danger
def moderation_words_add():
    body = body_or_empty()
    return json_ok(word=admin_mod.add_banned_word(g.admin_ctx, body.get('word'), reason_of(body)))


@admin_bp.route('/moderation/words', methods=['DELETE'])
@danger
def moderation_words_remove():
    body = body_or_empty()
    admin_mod.remove_banned_word(g.admin_ctx, body.get('word'), reason_of(body))
    return json_ok()


@admin_bp.route('/moderation/names', methods=['GET'])
def moderation_names():
    return json_ok(**admin_mod.recent_names(request.args))

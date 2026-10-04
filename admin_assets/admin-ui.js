'use strict';
// ===== ADMIN PANEL — komponensek =====
// Újrahasznosítható elemek: jelvények, gombok, kártyák, táblázat, lapozó, fülek, szűrős listanézet, űrlap-párbeszéd
// (kötelező indoklással és névbegépeléses megerősítéssel), grafikonok, tábla-rajzoló. Mindenhol DOM API.

const UI = {
    badge(text, kind = 'muted') {
        return h('span', { class: 'admin-badge badge-' + kind }, text);
    },

    btn(label, onClick, options = {}) {
        const classes = [];
        if (options.kind) classes.push(options.kind);
        if (options.small !== false) classes.push('small-btn');
        const button = h('button', { type: 'button', class: classes.join(' '), disabled: options.disabled,
            title: options.title }, label);
        if (onClick) button.addEventListener('click', onClick);
        return button;
    },

    link(label, hash, className) {
        return h('a', { class: className || 'admin-inline-link', href: hash }, label);
    },

    card(title, ...children) {
        return h('section', { class: 'admin-card' }, title ? h('h3', { class: 'admin-card-title' }, title) : null,
            children);
    },

    // Címsor + magyarázat + gombok egy nézet tetején
    header(title, note, ...actions) {
        return h('div', { class: 'admin-view-header' },
            h('div', null, h('h2', { class: 'section-title' }, title),
                note ? h('p', { class: 'form-hint form-hint-tight' }, note) : null),
            actions.length ? h('div', { class: 'admin-header-actions' }, actions) : null);
    },

    subtitle(text) {
        return h('h3', { class: 'subsection-title' }, text);
    },

    // Számláló-csempe (áttekintés): felirat, nagy szám, opcionális hivatkozás és szín
    stat(label, value, options = {}) {
        const tile = h(options.href ? 'a' : 'div', { class: 'admin-stat' + (options.kind ? ' stat-' + options.kind : ''),
            href: options.href });
        tile.appendChild(h('div', { class: 'admin-stat-value' }, value));
        tile.appendChild(h('div', { class: 'admin-stat-label' }, label));
        if (options.hint) tile.appendChild(h('div', { class: 'admin-stat-hint' }, options.hint));
        return tile;
    },

    // Címke–érték lista; az üres értékek kimaradnak
    kv(rows) {
        const list = h('dl', { class: 'admin-kv' });
        for (const [label, value] of rows) {
            if (value === null || value === undefined || value === '' || value === false) continue;
            list.appendChild(h('dt', null, label));
            list.appendChild(h('dd', null, value));
        }
        return list;
    },

    empty(text) {
        return h('div', { class: 'empty-state' }, h('p', { class: 'empty-msg' }, text));
    },

    loading() {
        return h('div', { class: 'admin-skeleton', 'aria-hidden': 'true' },
            h('div', { class: 'skeleton-line' }), h('div', { class: 'skeleton-line short' }),
            h('div', { class: 'skeleton-line' }));
    },

    errorBox(message, onRetry) {
        const box = h('div', { class: 'admin-error-box' }, h('p', { class: 'auth-error' }, message));
        if (onRetry) box.appendChild(UI.btn(t('admin.retry'), onRetry, { kind: 'secondary' }));
        return box;
    },

    // Táblázat: columns [{label, cell(item) → Node|szöveg, cls, sortKey}]; a sorok kattinthatók, ha van onRow
    table(config) {
        const { columns, items, onRow, rowClass } = config;
        const table = h('table', { class: 'admin-table admin-table-cards' + (onRow ? ' admin-table-clickable' : '')
            + (config.compact ? ' admin-table-compact' : '') });
        const head = h('tr');
        for (const column of columns) {
            const th = h('th', { class: column.cls });
            if (column.sortKey && config.onSort) {
                const active = config.sort && config.sort.key === column.sortKey;
                const arrow = active ? (config.sort.order === 'asc' ? ' ▲' : ' ▼') : '';
                const button = h('button', { type: 'button', class: 'admin-sort' + (active ? ' active' : '') },
                    column.label + arrow);
                button.addEventListener('click', () => config.onSort(column.sortKey));
                th.appendChild(button);
            } else {
                th.textContent = column.label;
            }
            head.appendChild(th);
        }
        table.appendChild(h('thead', null, head));
        const body = h('tbody');
        for (const item of items) {
            const row = h('tr', { class: rowClass ? rowClass(item) : null });
            for (const column of columns) {
                const content = column.cell(item);
                const cell = h('td', { class: column.cls, dataset: { label: column.label } });
                if (content !== null && content !== undefined && content !== false) appendChildren(cell, [content]);
                row.appendChild(cell);
            }
            if (onRow) {
                row.tabIndex = 0;
                row.addEventListener('click', (event) => {
                    if (event.target.closest('a, button, input, select, textarea, label')) return;
                    onRow(item);
                });
                row.addEventListener('keydown', (event) => {
                    if ((event.key === 'Enter' || event.key === ' ') && event.target === row) {
                        event.preventDefault();
                        onRow(item);
                    }
                });
            }
            body.appendChild(row);
        }
        table.appendChild(body);
        return h('div', { class: 'admin-table-wrap' }, table);
    },

    pager(total, offset, limit, onGo) {
        const shown = Math.min(limit, Math.max(0, total - offset));
        const prev = UI.btn(t('admin.prev'), () => onGo(Math.max(0, offset - limit)),
            { kind: 'secondary', disabled: offset <= 0 });
        const next = UI.btn(t('admin.next'), () => onGo(offset + limit),
            { kind: 'secondary', disabled: offset + shown >= total });
        return h('div', { class: 'admin-pager' }, prev,
            h('span', { class: 'text-secondary text-sm' },
                total ? t('admin.audit_range', { from: offset + 1, to: offset + shown, total }) : ''),
            next);
    },

    // Fülek: items [{id, label}]; onSelect(id)
    tabs(items, active, onSelect) {
        const bar = h('div', { class: 'admin-tabs', role: 'tablist' });
        for (const item of items) {
            const tab = h('button', { type: 'button', role: 'tab', class: 'admin-tab' + (item.id === active ? ' active' : ''),
                'aria-selected': item.id === active ? 'true' : 'false' }, item.label);
            tab.addEventListener('click', () => onSelect(item.id));
            bar.appendChild(tab);
        }
        return bar;
    },

    userLink(id, name) {
        if (id === null || id === undefined) return name || '';
        return UI.link(name || ('#' + id), Router.href('users', id));
    },

    gameLink(id, label) {
        return UI.link(label || ('#' + id), Router.href('games', id));
    },

    roomLink(key, label) {
        return UI.link(label || key, Router.href('rooms', key));
    },

    // Letöltési hivatkozás (CSV / JSON): az admin végpont a sütivel és a fejléc nélkül is működő GET
    download(label, path, query) {
        const link = h('a', { class: 'admin-link-btn', href: path + queryString(query) }, label);
        link.setAttribute('download', '');
        return link;
    },

    // Kis jelzőpont (online / offline)
    dot(on) {
        return h('span', { class: 'admin-dot ' + (on ? 'on' : 'off'), role: 'img',
            'aria-label': on ? t('admin.online') : t('admin.offline') });
    },

    // A játékbeli zseton kinézetű betű
    tile(letter, blank) {
        return h('span', { class: 'admin-tile' + (blank ? ' blank' : '') + (letter && letter.length > 1 ? ' long' : '') },
            letter === '?' ? '' : letter);
    },

    copy(text) {
        return UI.btn(t('admin.copy'), async () => {
            try {
                await navigator.clipboard.writeText(text);
                showToast(t('admin.copied'));
            } catch { showToast(text); }
        }, { kind: 'secondary' });
    },
};

// Betöltés vázlattal: a hiba esetén újrapróbálható. Visszatér: az adat, vagy null.
async function loadInto(container, fetcher, render) {
    container.replaceChildren(UI.loading());
    const result = await fetcher();
    // Közben másik nézetre léptek: a késve érkező válasz nem rajzol és nem regisztrál semmit
    if (!container.isConnected) return null;
    if (!result.ok) {
        container.replaceChildren(UI.errorBox(Api.message(result), () => loadInto(container, fetcher, render)));
        return null;
    }
    container.replaceChildren();
    render(container, result.data);
    return result.data;
}

// ----- Szűrős, lapozós, rendezhető listanézet (a szűrők az URL-ben: a vissza gomb és a linkmásolás működik) -----

function mountList(view, params, config) {
    const pageSize = config.pageSize || 50;
    const filterNames = (config.filters || []).map((f) => f.name);
    const state = {
        offset: Math.max(0, parseInt(params.get('offset') || '0', 10) || 0),
        sort: params.get('sort') || config.defaultSort || '',
        order: params.get('order') || config.defaultOrder || 'desc',
    };

    const activeFilters = () => {
        const query = new URLSearchParams();
        for (const name of filterNames) {
            const value = params.get(name);
            if (value) query.set(name, value);
        }
        return query;
    };

    // Az URL-ben maradó, nem szűrő paraméterek (pl. a fül): a szerver felé nem mennek
    const keepQuery = () => {
        const query = new URLSearchParams();
        for (const name of config.keep || []) if (params.get(name)) query.set(name, params.get(name));
        return query;
    };

    const go = (extra) => {
        const query = keepQuery();
        for (const [key, value] of activeFilters().entries()) query.set(key, value);
        if (state.sort) { query.set('sort', state.sort); query.set('order', state.order); }
        for (const [key, value] of Object.entries(extra || {})) {
            if (value === null || value === '' || value === undefined) query.delete(key); else query.set(key, value);
        }
        Router.go(config.section, '', query);
    };

    view.appendChild(UI.header(config.title, config.note,
        ...(config.headerActions || []),
        UI.btn(t('admin.refresh'), () => load(), { kind: 'secondary' })));
    if (config.banner) view.appendChild(config.banner);
    if (config.filters && config.filters.length) view.appendChild(renderFilterForm(config.filters, params, (query) => {
        for (const [key, value] of keepQuery().entries()) query.set(key, value);
        if (state.sort) { query.set('sort', state.sort); query.set('order', state.order); }
        Router.go(config.section, '', query);
    }, () => Router.go(config.section, '', keepQuery())));
    const results = h('div', { class: 'admin-results' });
    view.appendChild(results);

    async function load(silent) {
        const query = activeFilters();
        query.set('limit', String(pageSize));
        query.set('offset', String(state.offset));
        if (state.sort) { query.set('sort', state.sort); query.set('order', state.order); }
        if (!silent) results.replaceChildren(UI.loading());
        const result = await Api.get(config.path, query);
        if (!result.ok || !result.data[config.itemsKey || 'items']) {
            results.replaceChildren(UI.errorBox(Api.message(result), () => load()));
            return;
        }
        const data = result.data;
        const items = data[config.itemsKey || 'items'];
        results.replaceChildren();
        const total = data.total === undefined ? items.length : data.total;
        const bar = h('div', { class: 'admin-toolbar' },
            h('span', { class: 'text-secondary text-sm' },
                total ? t('admin.audit_range', { from: state.offset + 1, to: state.offset + items.length, total }) : ''),
            h('div', { class: 'admin-toolbar-actions' }, config.toolbar ? config.toolbar(data) : null,
                config.csv === false ? null : UI.download(t('admin.export_csv'), config.path,
                    new URLSearchParams([...activeFilters().entries(), ['format', 'csv']]))));
        results.appendChild(bar);
        if (config.summary) results.appendChild(config.summary(data));
        if (!items.length) {
            results.appendChild(UI.empty(config.empty || t('admin.list_empty')));
            return;
        }
        results.appendChild(UI.table({
            columns: config.columns, items, onRow: config.onRow, rowClass: config.rowClass, compact: config.compact,
            sort: state.sort ? { key: state.sort, order: state.order } : null,
            onSort: config.columns.some((c) => c.sortKey) ? (key) => {
                state.sort = key;
                state.order = params.get('sort') === key && params.get('order') === 'desc' ? 'asc' : 'desc';
                if (config.defaultOrder && !params.get('sort')) state.order = config.defaultOrder === 'desc' ? 'asc' : 'desc';
                go({ sort: state.sort, order: state.order, offset: null });
            } : null,
        }));
        if (total > pageSize) {
            results.appendChild(UI.pager(total, state.offset, pageSize, (offset) => go({ offset: offset || null })));
        }
        if (config.after) config.after(data, results);
    }

    load();
    return { reload: (silent) => load(silent) };
}

function renderFilterForm(filters, params, onApply, onReset) {
    const form = h('form', { class: 'admin-filters' });
    for (const filter of filters) {
        let control;
        if (filter.type === 'select') {
            control = h('select', { name: filter.name },
                filter.options.map((option) => h('option', { value: option.value }, option.label)));
            control.value = params.get(filter.name) || '';
        } else if (filter.type === 'checkbox') {
            control = h('input', { type: 'checkbox', name: filter.name, checked: params.get(filter.name) === '1' });
        } else {
            control = h('input', { type: filter.type || 'text', name: filter.name, autocomplete: 'off',
                value: params.get(filter.name) || '', placeholder: filter.placeholder || null });
        }
        const field = h('label', { class: 'admin-field' + (filter.type === 'checkbox' ? ' admin-field-check' : '') },
            h('span', { class: 'admin-field-label' }, filter.label), control);
        form.appendChild(field);
    }
    const apply = h('button', { type: 'submit', class: 'small-btn' }, t('admin.apply'));
    const reset = h('button', { type: 'button', class: 'secondary small-btn' }, t('admin.reset'));
    reset.addEventListener('click', onReset);
    form.appendChild(h('div', { class: 'admin-filter-actions' }, apply, reset));
    form.addEventListener('submit', (event) => {
        event.preventDefault();
        const query = new URLSearchParams();
        for (const filter of filters) {
            const control = form.elements[filter.name];
            const value = filter.type === 'checkbox' ? (control.checked ? '1' : '') : control.value.trim();
            if (value) query.set(filter.name, value);
        }
        onApply(query);
    });
    return form;
}

// ----- Általános (információs) párbeszéd: a `#admin-detail-dialog` újrahasznosítva -----

const Modal = {
    show(title, content, options = {}) {
        document.getElementById('admin-detail-title').textContent = title;
        const body = document.getElementById('admin-detail-body');
        body.replaceChildren();
        appendChildren(body, [content]);
        document.getElementById('admin-detail-box').classList.toggle('admin-detail-wide', !!options.wide);
        Dialogs.open('admin-detail-dialog', false);
    },
    close() { Dialogs.close('admin-detail-dialog'); },
};

// ----- Űrlap-párbeszéd: minden módosító művelet ezen megy át (kötelező indoklás, célpont-név megerősítés) -----
//
// Act.open({title, text, fields: [...], reason: true, confirmName: 'név', submit: 'Címke', danger: true,
//           run: (values) => Api.post(...), done: (data) => {...}})
// mezők: {name, type: text|textarea|select|number|checkbox|date|datetime|password|check_list, label, hint, value,
//         options: [{value, label}], required, min, max, rows, placeholder, maxlength}

const Act = {
    _config: null,
    _busy: false,

    open(config) {
        this._config = config;
        document.getElementById('admin-form-title').textContent = config.title || '';
        const text = document.getElementById('admin-form-text');
        text.textContent = config.text || '';
        text.classList.toggle('hidden', !config.text);
        const fields = document.getElementById('admin-form-fields');
        fields.replaceChildren();
        for (const field of config.fields || []) fields.appendChild(this.buildField(field));
        if (config.confirmName) {
            fields.appendChild(this.buildField({
                name: 'confirm_name', type: 'text', required: true,
                label: t('admin.type_name', { name: config.confirmName }), placeholder: config.confirmName,
            }));
        }
        if (config.reason !== false) {
            const optional = config.reason === 'optional';
            fields.appendChild(this.buildField({
                name: 'reason', type: 'textarea', required: !optional, rows: 2, maxlength: 500,
                label: optional ? t('admin.reason_optional') : t('admin.reason'),
                hint: optional ? null : t('admin.reason_hint'), value: config.reasonValue || '',
            }));
        }
        const submit = document.getElementById('admin-form-submit');
        submit.textContent = config.submit || t('admin.confirm');
        submit.classList.toggle('danger', !!config.danger);
        submit.disabled = false;
        hideFormError('admin-form-error');
        document.getElementById('admin-form-box').classList.toggle('admin-form-wide', !!config.wide);
        Dialogs.open('admin-form-dialog', false);
        const first = fields.querySelector('input:not([type=checkbox]), textarea, select');
        if (first) first.focus();
    },

    close() {
        Dialogs.close('admin-form-dialog');
        this._config = null;
    },

    buildField(field) {
        const id = 'admin-field-' + field.name;
        let control;
        if (field.type === 'textarea') {
            control = h('textarea', { id, name: field.name, rows: field.rows || 3, maxlength: field.maxlength || null,
                placeholder: field.placeholder || null });
            control.value = field.value || '';
        } else if (field.type === 'select') {
            control = h('select', { id, name: field.name },
                field.options.map((option) => h('option', { value: String(option.value) }, option.label)));
            if (field.value !== undefined && field.value !== null) control.value = String(field.value);
        } else if (field.type === 'checkbox') {
            control = h('input', { id, name: field.name, type: 'checkbox', checked: !!field.value });
        } else if (field.type === 'check_list') {
            control = h('div', { class: 'admin-check-list', id });
            for (const option of field.options) {
                control.appendChild(h('label', { class: 'admin-check-row' },
                    h('input', { type: 'checkbox', value: String(option.value), checked: !!option.checked }),
                    h('span', null, option.label)));
            }
        } else {
            const type = field.type === 'datetime' ? 'datetime-local' : (field.type || 'text');
            control = h('input', { id, name: field.name, type, autocomplete: 'off', placeholder: field.placeholder || null,
                min: field.min !== undefined ? field.min : null, max: field.max !== undefined ? field.max : null,
                maxlength: field.maxlength || null, step: field.step || null });
            if (field.value !== undefined && field.value !== null) control.value = String(field.value);
        }
        if (field.required && field.type !== 'checkbox' && field.type !== 'check_list') control.required = true;
        control.dataset.fieldType = field.type || 'text';
        if (field.options) control.dataset.hasOptions = '1';
        control._field = field;
        const wrap = h('div', { class: 'admin-form-field' + (field.type === 'checkbox' ? ' admin-form-check' : '') });
        if (field.type === 'checkbox') {
            wrap.appendChild(h('label', { class: 'admin-check-row', for: id }, control, h('span', null, field.label)));
        } else {
            wrap.appendChild(h('label', { class: 'admin-field-label', for: id }, field.label));
            wrap.appendChild(control);
        }
        if (field.hint) wrap.appendChild(h('div', { class: 'form-hint form-hint-tight' }, field.hint));
        wrap.appendChild(h('div', { class: 'admin-field-error hidden', 'data-error-for': field.name }));
        return wrap;
    },

    values() {
        const values = {};
        const form = document.getElementById('admin-form');
        for (const control of form.querySelectorAll('[name]')) {
            const field = control._field || {};
            const name = control.name;
            if (control.type === 'checkbox') values[name] = control.checked;
            else if (field.type === 'number') values[name] = control.value === '' ? null : Number(control.value);
            else if (field.type === 'select' && field.options) {
                const match = field.options.find((option) => String(option.value) === control.value);
                values[name] = match ? match.value : control.value;
            } else values[name] = control.value.trim();
        }
        for (const list of form.querySelectorAll('.admin-check-list')) {
            values[list._field.name] = Array.from(list.querySelectorAll('input:checked')).map((input) => input.value);
        }
        return values;
    },

    async submit(event) {
        event.preventDefault();
        const config = this._config;
        if (!config || this._busy) return;
        const values = this.values();
        const form = document.getElementById('admin-form');
        for (const node of form.querySelectorAll('.admin-field-error')) node.classList.add('hidden');
        for (const node of form.querySelectorAll('.invalid')) node.classList.remove('invalid');
        hideFormError('admin-form-error');
        if (config.confirmName && values.confirm_name !== config.confirmName) {
            showFormError('admin-form-error', t('admin.err_name_mismatch'));
            return;
        }
        if (config.validate) {
            const problem = config.validate(values);
            if (problem) { showFormError('admin-form-error', problem); return; }
        }
        const button = document.getElementById('admin-form-submit');
        button.disabled = true;
        this._busy = true;
        try {
            const result = await config.run(values);
            if (result.ok) {
                const done = config.done;
                const doneText = config.doneText;
                this.close();
                showToast(doneText || t('admin.done'));
                if (done) done(result.data, values);
            } else if (result.status !== 401) {
                const message = Api.message(result);
                showFormError('admin-form-error', message);
                const field = result.data.field;
                const control = field ? form.elements[field] : null;
                if (control) {
                    control.classList.add('invalid');
                    const slot = form.querySelector('[data-error-for="' + field + '"]');
                    if (slot) { slot.textContent = message; slot.classList.remove('hidden'); }
                }
            }
        } finally {
            button.disabled = false;
            this._busy = false;
        }
    },
};

// A tiltás / némítás időtartamai (a szerver 1h / 1d / 7d / 30d / permanent értékeket ismer)
function durationOptions(permanent = true) {
    const options = [
        { value: '1h', label: t('admin.dur_opt_hour') },
        { value: '1d', label: t('admin.dur_opt_day') },
        { value: '7d', label: t('admin.dur_opt_week') },
        { value: '30d', label: t('admin.dur_opt_month') },
    ];
    if (permanent) options.push({ value: 'permanent', label: t('admin.dur_opt_permanent') });
    return options;
}

// ----- Grafikonok (SVG) -----

const Chart = {
    // spec: {days: [...], series: [{label, values: [...], cls}], type: 'bar'|'line'|'stack', yLabel}
    render(spec) {
        const width = 640;
        const height = 220;
        const pad = { left: 38, right: 8, top: 10, bottom: 22 };
        const days = spec.days;
        const count = Math.max(1, days.length);
        const stacked = spec.type === 'stack';
        const totals = days.map((_, i) => stacked
            ? spec.series.reduce((sum, s) => sum + (s.values[i] || 0), 0)
            : Math.max(0, ...spec.series.map((s) => s.values[i] || 0)));
        const max = Math.max(1, ...totals);
        const innerW = width - pad.left - pad.right;
        const innerH = height - pad.top - pad.bottom;
        const svg = svgEl('svg', { viewBox: `0 0 ${width} ${height}`, class: 'admin-chart', role: 'img' });
        for (const fraction of [0, 0.5, 1]) {
            const y = pad.top + innerH * (1 - fraction);
            svg.appendChild(svgEl('line', { x1: pad.left, x2: width - pad.right, y1: y, y2: y, class: 'chart-grid' }));
            svg.appendChild(svgEl('text', { x: pad.left - 5, y: y + 3, class: 'chart-axis', 'text-anchor': 'end' },
                fmtNum(Math.round(max * fraction * 10) / 10, 1)));
        }
        const step = innerW / count;
        if (spec.type === 'line') {
            spec.series.forEach((series, index) => {
                const points = days.map((_, i) => {
                    const x = pad.left + step * (i + 0.5);
                    const y = pad.top + innerH * (1 - (series.values[i] || 0) / max);
                    return x.toFixed(1) + ',' + y.toFixed(1);
                });
                svg.appendChild(svgEl('polyline', { points: points.join(' '), class: 'chart-line chart-s' + (series.cls ?? index) }));
            });
            days.forEach((day, i) => {
                const x = pad.left + step * i;
                const dot = svgEl('rect', { x, y: pad.top, width: step, height: innerH, class: 'chart-hit' });
                dot.appendChild(svgEl('title', null,
                    day + ': ' + spec.series.map((s) => s.label + ' ' + fmtNum(s.values[i] || 0, 1)).join(' · ')));
                svg.appendChild(dot);
            });
        } else {
            days.forEach((day, i) => {
                let base = 0;
                const tip = day + ': ' + spec.series.map((s) => s.label + ' ' + fmtNum(s.values[i] || 0, 1)).join(' · ');
                spec.series.forEach((series, index) => {
                    const value = series.values[i] || 0;
                    const barH = innerH * value / max;
                    const x = pad.left + step * i + step * 0.12;
                    const w = stacked ? step * 0.76 : step * 0.76 / spec.series.length;
                    const xx = stacked ? x : x + w * index;
                    const y = pad.top + innerH - barH - (stacked ? innerH * base / max : 0);
                    const rect = svgEl('rect', { x: xx.toFixed(1), y: y.toFixed(1), width: Math.max(0.5, w - 0.5).toFixed(1),
                        height: Math.max(0, barH).toFixed(1), rx: 1.5, class: 'chart-bar chart-s' + (series.cls ?? index) });
                    rect.appendChild(svgEl('title', null, tip));
                    svg.appendChild(rect);
                    if (stacked) base += value;
                });
            });
        }
        const labelIdx = Array.from(new Set([0, Math.floor((count - 1) / 2), count - 1]));
        for (const i of labelIdx) {
            if (!days[i]) continue;
            svg.appendChild(svgEl('text', { x: pad.left + step * (i + 0.5), y: height - 6, class: 'chart-axis',
                'text-anchor': i === 0 ? 'start' : (i === count - 1 ? 'end' : 'middle') }, formatDay(days[i])));
        }
        const legend = h('div', { class: 'admin-legend' });
        spec.series.forEach((series, index) => {
            legend.appendChild(h('span', { class: 'admin-legend-item' },
                h('span', { class: 'legend-swatch chart-s' + (series.cls ?? index) }), series.label));
        });
        return h('figure', { class: 'admin-figure' }, spec.title ? h('figcaption', null, spec.title) : null, svg, legend);
    },

    // 7 × 24-es hőtérkép (hét napja × óra)
    heatmap(grid, max, dayLabels) {
        const wrap = h('div', { class: 'admin-heatmap' });
        wrap.appendChild(h('div', { class: 'heat-corner' }));
        for (let hour = 0; hour < 24; hour += 1) {
            wrap.appendChild(h('div', { class: 'heat-hour' }, hour % 3 === 0 ? String(hour) : ''));
        }
        grid.forEach((row, day) => {
            wrap.appendChild(h('div', { class: 'heat-day' }, dayLabels[day] || ''));
            row.forEach((value, hour) => {
                const level = value <= 0 ? 0 : Math.min(5, 1 + Math.floor(5 * value / Math.max(1, max + 1)));
                wrap.appendChild(h('div', { class: 'heat-cell heat-' + level, title: dayLabels[day] + ' ' + hour + ':00 — ' + value }));
            });
        });
        return wrap;
    },

    // Vízszintes oszlopok címkékkel: items [{label, value}]
    hbars(items, formatValue) {
        const max = Math.max(1, ...items.map((item) => item.value));
        const wrap = h('div', { class: 'admin-hbars' });
        for (const item of items) {
            const bar = h('div', { class: 'hbar-fill' });
            bar.style.width = Math.max(1, Math.round(100 * item.value / max)) + '%';
            wrap.appendChild(h('div', { class: 'hbar-row' }, h('span', { class: 'hbar-label' }, item.label),
                h('div', { class: 'hbar-track' }, bar),
                h('span', { class: 'hbar-value' }, formatValue ? formatValue(item.value) : fmtNum(item.value))));
        }
        return wrap;
    },
};

// ----- Tábla (15×15) -----

const BOARD_PREMIUM = (() => {
    const quarter = [
        [0, 0, 'tw'], [0, 3, 'dl'], [0, 7, 'tw'], [1, 1, 'dw'], [1, 5, 'tl'], [2, 2, 'dw'], [2, 6, 'dl'],
        [3, 0, 'dl'], [3, 3, 'dw'], [3, 7, 'dl'], [4, 4, 'dw'], [5, 1, 'tl'], [5, 5, 'tl'], [6, 2, 'dl'],
        [6, 6, 'dl'], [7, 0, 'tw'], [7, 3, 'dl'], [7, 7, 'st'],
    ];
    const map = {};
    for (const [r, c, kind] of quarter) {
        for (const [rr, cc] of [[r, c], [r, 14 - c], [14 - r, c], [14 - r, 14 - c]]) map[rr + ',' + cc] = kind;
    }
    return map;
})();

// cells: 15×15 (null | {letter, is_blank}); highlight: [{row, col}] (az utolsó lépés)
function renderBoard(cells, highlight) {
    const marked = new Set((highlight || []).map((p) => p.row + ',' + p.col));
    const board = h('div', { class: 'admin-board', role: 'img', 'aria-label': t('admin.board') });
    for (let row = 0; row < 15; row += 1) {
        for (let col = 0; col < 15; col += 1) {
            const cell = cells && cells[row] ? cells[row][col] : null;
            const premium = BOARD_PREMIUM[row + ',' + col];
            const node = h('div', { class: 'admin-bc' + (premium ? ' bc-' + premium : '') + (marked.has(row + ',' + col) ? ' bc-last' : '') });
            if (cell) node.appendChild(UI.tile(cell.letter, cell.is_blank));
            board.appendChild(node);
        }
    }
    return board;
}

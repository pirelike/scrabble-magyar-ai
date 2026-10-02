// ===== I18N (többnyelvű felület) =====
// A fordítások a `i18n-data.js`-ben vannak ({hu: {...}, en: {...}}); a magyar az alapnyelv.
//   t('kulcs', {név: érték})   — szöveg a mindenkori nyelven; {név} helyére a paraméter kerül.
//                                Ha van `kulcs_one` és a paraméter `n` értéke 1, azt használja (angol egyes szám).
//   data-i18n="kulcs"          — az elem szövege
//   data-i18n-placeholder      — a beviteli mező helyőrzője
//   data-i18n-title            — az elem címkéje (title, és ha van, aria-label is)
//   data-i18n-aria             — csak a képernyőolvasónak szánt címke (aria-label)
// A szerver magyar üzeneteit az `I18N.tServer()` fordítja (pontos egyezés vagy minta alapján).

const I18N = {
    SUPPORTED: ['hu', 'en'],
    lang: 'hu',
    data: window.I18N_DATA || { hu: {}, en: {} },

    init() {
        const attr = document.documentElement.getAttribute('lang');
        this.lang = this.SUPPORTED.includes(attr) ? attr : 'hu';
        this.apply();
    },

    t(key, params) {
        let str = (this.data[this.lang] || {})[key];
        if (str === undefined) str = (this.data.hu || {})[key];
        if (str === undefined) return key;
        if (params && params.n === 1) {
            const one = (this.data[this.lang] || {})[key + '_one'];
            if (one !== undefined) str = one;
        }
        if (params) {
            str = str.replace(/\{(\w+)\}/g, (match, name) => (params[name] !== undefined ? params[name] : match));
        }
        return str;
    },

    has(key) {
        return (this.data[this.lang] || {})[key] !== undefined;
    },

    // Dátumok és számok nyelvi beállítása
    locale() {
        return this.lang === 'hu' ? 'hu-HU' : 'en-GB';
    },

    // A DOM statikus szövegeinek fordítása
    apply(root = document) {
        root.querySelectorAll('[data-i18n]').forEach((el) => {
            el.textContent = this.t(el.dataset.i18n);
        });
        root.querySelectorAll('[data-i18n-placeholder]').forEach((el) => {
            el.setAttribute('placeholder', this.t(el.dataset.i18nPlaceholder));
        });
        root.querySelectorAll('[data-i18n-title]').forEach((el) => {
            const text = this.t(el.dataset.i18nTitle);
            el.setAttribute('title', text);
            if (el.hasAttribute('aria-label')) el.setAttribute('aria-label', text);
        });
        root.querySelectorAll('[data-i18n-aria]').forEach((el) => {
            el.setAttribute('aria-label', this.t(el.dataset.i18nAria));
        });
        document.querySelectorAll('.lang-code').forEach((el) => {
            el.textContent = this.lang.toUpperCase();
        });
        document.title = this.t('app.title');
        const description = document.querySelector('meta[name="description"]');
        if (description) description.setAttribute('content', this.t('app.description'));
    },

    setLang(lang) {
        if (!this.SUPPORTED.includes(lang) || lang === this.lang) return;
        this.lang = lang;
        document.documentElement.setAttribute('lang', lang);
        try { localStorage.setItem('scrabble-lang', lang); } catch { /* localStorage tiltva */ }
        this.apply();
        window.dispatchEvent(new CustomEvent('langchange', { detail: { lang } }));
    },

    // A következő nyelv (hu → en → hu ...)
    next() {
        const i = this.SUPPORTED.indexOf(this.lang);
        return this.SUPPORTED[(i + 1) % this.SUPPORTED.length];
    },

    // A szerver magyar üzenetének fordítása. Ismeretlen üzenet változatlanul marad.
    tServer(message) {
        if (typeof message !== 'string' || !message || this.lang === 'hu') return message;
        const server = (this.data[this.lang] || {}).server;
        if (!server) return message;
        if (Object.prototype.hasOwnProperty.call(server.exact, message)) return server.exact[message];
        for (const [pattern, template] of server.patterns) {
            const match = new RegExp(pattern).exec(message);
            if (match) {
                return template.replace(/\$(\d)/g, (m, i) => (match[Number(i)] !== undefined ? match[Number(i)] : m));
            }
        }
        return message;
    },
};

function t(key, params) { return I18N.t(key, params); }
function tServer(message) { return I18N.tServer(message); }

I18N.init();

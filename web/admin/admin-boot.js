// Az admin oldal indítása: a téma és a nyelv a megjelenítés előtt áll be (nincs villanás).
// Külön fájl, mert az admin oldal szigorú Content-Security-Policy-je a beágyazott szkriptet tiltja.
// A kulcsok megegyeznek a fő alkalmazáséval, így a választás közös.
(function () {
    var theme = 'light';
    try {
        var saved = localStorage.getItem('scrabble-theme');
        theme = (saved === 'light' || saved === 'dark')
            ? saved
            : (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
    } catch (e) { /* localStorage tiltva */ }
    document.documentElement.setAttribute('data-theme', theme);
    var lang = 'hu';
    try {
        var savedLang = localStorage.getItem('scrabble-lang');
        lang = (savedLang === 'hu' || savedLang === 'en')
            ? savedLang
            : (((navigator.language || 'hu').toLowerCase().indexOf('hu') === 0) ? 'hu' : 'en');
    } catch (e) { /* localStorage tiltva */ }
    document.documentElement.setAttribute('lang', lang);
})();

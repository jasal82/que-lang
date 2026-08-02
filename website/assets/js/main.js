(function () {
    'use strict';

    /* ---- mobile nav (landing header) ---------------------------------- */
    var navToggle = document.querySelector('[data-nav-toggle]');
    var siteNav = document.querySelector('.site-nav');
    if (navToggle && siteNav) {
        navToggle.addEventListener('click', function () {
            var open = siteNav.classList.toggle('is-open-mobile');
            navToggle.setAttribute('aria-expanded', open ? 'true' : 'false');
        });
    }

    /* ---- install method tabs ------------------------------------------- */
    var tabs = document.querySelectorAll('.install-tab');
    var panes = document.querySelectorAll('.install-pane');
    tabs.forEach(function (tab) {
        tab.addEventListener('click', function () {
            var target = tab.getAttribute('data-tab');
            tabs.forEach(function (t) { t.classList.toggle('is-active', t === tab); });
            panes.forEach(function (p) {
                p.classList.toggle('is-active', p.getAttribute('data-pane') === target);
            });
        });
    });

    /* ---- docs: mobile sidebar toggle ------------------------------------ */
    var sidebar = document.querySelector('.docs-sidebar');
    var scrim = document.querySelector('.docs-scrim');
    var openBtn = document.querySelector('[data-docs-open]');
    function closeSidebar() {
        if (sidebar) sidebar.classList.remove('is-open');
        if (scrim) scrim.classList.remove('is-open');
    }
    if (openBtn && sidebar) {
        openBtn.addEventListener('click', function () {
            sidebar.classList.add('is-open');
            if (scrim) scrim.classList.add('is-open');
        });
    }
    if (scrim) scrim.addEventListener('click', closeSidebar);
    document.querySelectorAll('.docs-nav-list a').forEach(function (a) {
        a.addEventListener('click', closeSidebar);
    });

    /* ---- docs: sidebar filter ------------------------------------------- */
    var search = document.querySelector('.docs-search');
    if (search) {
        var items = Array.prototype.slice.call(document.querySelectorAll('.docs-nav-list li'));
        var parts = Array.prototype.slice.call(document.querySelectorAll('.docs-part'));
        search.addEventListener('input', function () {
            var q = search.value.trim().toLowerCase();
            items.forEach(function (li) {
                var text = li.textContent.toLowerCase();
                li.style.display = (!q || text.indexOf(q) !== -1) ? '' : 'none';
            });
            parts.forEach(function (part) {
                var list = part.nextElementSibling;
                if (!list) return;
                var anyVisible = Array.prototype.slice.call(list.children)
                    .some(function (li) { return li.style.display !== 'none'; });
                part.style.display = (!q || anyVisible) ? '' : 'none';
            });
        });
        search.addEventListener('keydown', function (e) {
            if (e.key === 'Escape') { search.value = ''; search.dispatchEvent(new Event('input')); search.blur(); }
        });
        document.addEventListener('keydown', function (e) {
            if (e.key === '/' && document.activeElement !== search) {
                e.preventDefault();
                search.focus();
            }
        });
    }

    /* ---- docs: scrollspy for sidebar + right rail ------------------------ */
    var headings = Array.prototype.slice.call(document.querySelectorAll('.docs-content h2[id], .docs-content h3[id]'));
    var sideLinks = Array.prototype.slice.call(document.querySelectorAll('.docs-nav-list a'));
    var railLinks = Array.prototype.slice.call(document.querySelectorAll('.docs-rail a'));

    function setActive(id) {
        sideLinks.forEach(function (a) {
            a.classList.toggle('is-active', a.getAttribute('href') === '#' + id);
        });
        railLinks.forEach(function (a) {
            a.classList.toggle('is-active', a.getAttribute('href') === '#' + id);
        });
    }

    if (headings.length && 'IntersectionObserver' in window) {
        var current = null;
        var io = new IntersectionObserver(function (entries) {
            entries.forEach(function (entry) {
                if (entry.isIntersecting) {
                    current = entry.target.id;
                }
            });
            if (current) setActive(current);
        }, { rootMargin: '-84px 0px -70% 0px', threshold: 0 });
        headings.forEach(function (h) { io.observe(h); });
    }

    /* ---- copy buttons (fallback if Prism toolbar plugin is absent) ------- */
    document.querySelectorAll('pre[class*="language-"]').forEach(function (pre) {
        if (pre.closest('.code-toolbar')) return; // handled by Prism plugin
        var btn = document.createElement('button');
        btn.className = 'manual-copy-btn';
        btn.type = 'button';
        btn.textContent = 'Copy';
        btn.addEventListener('click', function () {
            var code = pre.querySelector('code');
            navigator.clipboard.writeText(code ? code.textContent : pre.textContent).then(function () {
                btn.textContent = 'Copied';
                setTimeout(function () { btn.textContent = 'Copy'; }, 1500);
            });
        });
        pre.style.position = 'relative';
        pre.appendChild(btn);
    });

    /* ---- smooth scroll, but only for clicks after the page has settled ----
       (scroll-behavior:smooth applied from the start would also animate the
       browser's own initial jump to a URL fragment, which on a page this
       long turns a direct link to a late chapter into a multi-second
       scroll). ------------------------------------------------------------ */
    window.addEventListener('load', function () {
        requestAnimationFrame(function () {
            document.documentElement.classList.add('smooth-scroll');
        });
    });

    /* ---- current year in footer ------------------------------------------- */
    document.querySelectorAll('[data-year]').forEach(function (el) {
        el.textContent = new Date().getFullYear();
    });
})();

(() => {
  'use strict';

  const root = document.documentElement;
  const themeButton = document.querySelector('.theme-toggle');
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)');
  const isDark = () => root.dataset.theme ? root.dataset.theme === 'dark' : prefersDark.matches;
  const labelTheme = () => {
    themeButton.setAttribute('aria-label', `Switch to ${isDark() ? 'light' : 'dark'} theme`);
    document.querySelectorAll('.editor-preview source').forEach(source => { source.media = isDark() ? 'all' : 'not all'; });
  };
  labelTheme();
  themeButton.addEventListener('click', () => {
    root.dataset.theme = isDark() ? 'light' : 'dark';
    try { localStorage.setItem('hedit-docs-theme', root.dataset.theme); } catch (_) {}
    labelTheme();
  });
  prefersDark.addEventListener('change', labelTheme);

  document.querySelectorAll('.prose :is(h2, h3, h4)[id]').forEach(heading => {
    const link = document.createElement('a');
    link.className = 'heading-anchor';
    link.href = '#' + heading.id;
    link.setAttribute('aria-label', `Link to ${heading.textContent}`);
    link.textContent = '#';
    heading.append(link);
  });

  document.querySelectorAll('pre').forEach(pre => {
    const code = pre.querySelector('code');
    if (!code || !navigator.clipboard) return;
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'copy-code';
    button.textContent = 'Copy';
    button.setAttribute('aria-label', 'Copy code');
    button.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(code.textContent);
        button.textContent = 'Copied';
      } catch (_) {
        button.textContent = 'Select code to copy';
      }
      setTimeout(() => { button.textContent = 'Copy'; }, 2000);
    });
    pre.append(button);
  });

  const dialog = document.querySelector('.search-dialog');
  const dialogForm = document.querySelector('[data-dialog-search]');
  const pageForm = document.querySelector('[data-page-search]');
  const basePath = document.body.dataset.basePath || '';
  const indexURL = document.body.dataset.index;
  let indexPromise;

  function loadIndex() {
    if (!indexPromise) {
      indexPromise = new Promise((resolve, reject) => {
        const script = document.createElement('script');
        script.src = indexURL;
        script.onload = () => {
          try {
            const documents = window.searchIndex.documentStore.docs;
            resolve(Object.entries(documents).map(([url, entry]) => ({
              url, ...entry,
              titleLower: entry.title.toLocaleLowerCase(),
              descriptionLower: (entry.description || '').toLocaleLowerCase(),
              bodyLower: entry.body.toLocaleLowerCase()
            })));
          } catch (error) { reject(error); }
        };
        script.onerror = reject;
        document.head.append(script);
      }).catch(error => { indexPromise = undefined; throw error; });
    }
    return indexPromise;
  }

  function findMatches(entries, query) {
    const terms = query.toLocaleLowerCase().trim().split(/\s+/).slice(0, 8);
    return entries.map(entry => {
      let score = 0;
      for (const term of terms) {
        if (entry.titleLower.includes(term)) score += 12;
        else if (entry.descriptionLower.includes(term)) score += 5;
        else if (entry.bodyLower.includes(term)) score += 1;
        else return null;
      }
      return {entry, score};
    }).filter(Boolean).sort((a, b) => b.score - a.score || a.entry.title.localeCompare(b.entry.title)).slice(0, 16);
  }

  function setupSearch(form) {
    const input = form.querySelector('input');
    const status = form.querySelector('.search-status');
    const results = form.querySelector('.search-results');
    let generation = 0;
    let timer;

    async function run() {
      const request = ++generation;
      const query = input.value.trim().slice(0, 200);
      results.replaceChildren();
      if (!query) {
        status.textContent = 'Type to search the user guides and plugin reference.';
        return;
      }
      status.textContent = 'Searching…';
      try {
        const entries = await loadIndex();
        if (request !== generation) return;
        const matches = findMatches(entries, query);
        status.textContent = matches.length ? `${matches.length} result${matches.length === 1 ? '' : 's'}.` : `No results for “${query}”. Try a shorter phrase or an API name.`;
        for (const {entry} of matches) {
          const item = document.createElement('li');
          const link = document.createElement('a');
          const title = document.createElement('strong');
          const snippet = document.createElement('p');
          const section = document.createElement('small');
          link.href = basePath + entry.url;
          section.textContent = ({'guide': 'User guide', 'getting-started': 'Getting started', 'plugins': 'Plugin development', 'reference': 'API reference', 'project': 'Project'})[entry.url.split('/')[1]] || 'Documentation';
          title.textContent = entry.title;
          const firstTerm = query.toLocaleLowerCase().split(/\s+/)[0];
          const position = entry.bodyLower.indexOf(firstTerm);
          const start = Math.max(0, position - 60);
          snippet.textContent = entry.description || ((start ? '…' : '') + entry.body.slice(start, start + 220) + '…');
          link.append(section, title, snippet);
          item.append(link);
          results.append(item);
        }
      } catch (_) {
        if (request === generation) status.textContent = 'Search could not load. Try again, or browse Contents.';
      }
    }

    input.addEventListener('input', () => { generation++; clearTimeout(timer); timer = setTimeout(run, 100); });
    form.addEventListener('submit', event => { event.preventDefault(); clearTimeout(timer); run(); });
    form.addEventListener('keydown', event => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      const links = [...results.querySelectorAll('a')];
      if (!links.length) return;
      event.preventDefault();
      const position = links.indexOf(document.activeElement);
      const next = position + (event.key === 'ArrowDown' ? 1 : -1);
      if (next < 0) input.focus(); else links[Math.min(next, links.length - 1)].focus();
    });
    return {input, run};
  }

  const dialogSearch = setupSearch(dialogForm);
  if (pageForm) {
    const search = setupSearch(pageForm);
    search.input.value = new URLSearchParams(location.search).get('q') || '';
    if (search.input.value) search.run();
  }
  const openSearch = () => {
    if (typeof dialog.showModal !== 'function') return false;
    if (!dialog.open) dialog.showModal();
    dialogSearch.input.focus();
    dialogSearch.input.select();
    return true;
  };
  document.querySelectorAll('[data-open-search]').forEach(link => link.addEventListener('click', event => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) return;
    if (openSearch()) event.preventDefault();
  }));
  document.querySelector('[data-close-search]').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => {
    const bounds = dialog.getBoundingClientRect();
    if (event.target === dialog && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) dialog.close();
  });
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && dialog.open) {
      event.preventDefault();
      dialog.close();
      return;
    }
    const editing = event.target.closest('input, textarea, select, [contenteditable="true"]');
    if (((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') || (event.key === '/' && !editing && !event.metaKey && !event.ctrlKey && !event.altKey)) {
      if (openSearch()) event.preventDefault();
    }
  });
  document.querySelectorAll('[data-search-key]').forEach(key => { key.textContent = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘ K' : 'Ctrl K'; });
})();

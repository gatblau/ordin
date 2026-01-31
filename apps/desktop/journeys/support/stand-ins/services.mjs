// Stand-ins for the online services, on 127.0.0.1:9440, for the journeys (OrdinDesktop.md DSK-JRN-4). It
// runs in the folder STAND_IN_DIR (the run folder), where it writes services.log and services-pages.json.
//// Confluence: Atlassian's gateway (`/oauth/token/accessible-resources`, `/ex/confluence/<cloud>/wiki/...`)
// for one space PROC with a parent page "Supplier pages" (4000) and a child page "Rates" (4001). Pages
// live in memory; a PUT keeps the new body and raises the version. Every request is logged, and an
// unknown one answers 404 so the journey log shows what the adapter asked for.
//
// SharePoint: Microsoft Graph under `/graph/v1.0` (journeys-stand-ins.json `sharepoint.graph`), and the
// Microsoft token endpoint `POST /microsoft/oauth2/v2.0/token` (`sharepoint.token_endpoint`). One site
// "Procurement" (site-proc) with one library "Documents" (drive b!procdocs) holding the folder
// "Supplier contracts", which holds "Kestrel terms.docx" (a real, minimal Word file) and "Notes.md".
// Items live in memory: a content PUT checks If-Match (412 on a stale eTag), keeps the new bytes and
// raises the version, eTag and cTag; delta links carry a change counter, so a later delta shows edits.
// Upload sessions (files over 4 MB) answer at `/graph/__upload/<id>`.
//
// Jira: the REST API under `/rest/api/3` (API-token sign-in, `jira.site_base_url`) and under
// `/ex/jira/cloud-walk/rest/api/3` (OAuth, `jira.gateway_base_url`); site acme.atlassian.net. One project
// PROC "Procurement" with issue PROC-12 "Renew Kestrel contract", a description, no comments at first,
// and one attachment "quote.txt". Comments, labels, assignee and new issues are kept in memory.
//
// `/oauth/token/accessible-resources` answers both products with one resource (Confluence matches the
// URL; Jira also needs the `read:jira-work` scope).
import http from 'node:http';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';

const CLOUD = 'cloud-walk';
const SITE = 'https://acme.atlassian.net';
process.chdir(process.env.STAND_IN_DIR ?? '.');
const log = (line) => appendFileSync('services.log', `${new Date().toISOString()} ${line}\n`);

const version = (n) => ({ number: n, authorId: 'acc-1', createdAt: '2026-01-29T09:00:00.000Z' });
const pages = new Map([
  ['4000', { id: '4000', title: 'Supplier pages', spaceId: '65010', parentId: null, v: 3, body: '<p>Pages about our suppliers.</p>' }],
  ['4001', { id: '4001', title: 'Rates', spaceId: '65010', parentId: '4000', v: 2, body: '<h1>Rates</h1><p>Kestrel Freight: 1,180 GBP per container.</p>' }],
]);
let nextId = 4100;
// Confluence pages survive a restart in standin-pages.json, so a link made before a
// restart of the stand-in still matches its versions. A run starts without the file.
const PAGES_FILE = 'services-pages.json';
if (existsSync(PAGES_FILE)) {
  const saved = JSON.parse(readFileSync(PAGES_FILE, 'utf8'));
  pages.clear();
  for (const p of saved.pages) pages.set(p.id, p);
  nextId = saved.nextId;
}
const savePages = () => writeFileSync(PAGES_FILE, JSON.stringify({ nextId, pages: [...pages.values()] }, null, 1));
// The journeys read the pages from the file, so it exists from the start.
savePages();
// `POST /__fail_puts?n=<count>`: the next <count> page updates or SharePoint uploads answer 503, as a
// service that is down. Graph's adapter retries a 503 three times, so n=4 makes one upload fail outright.
let failPuts = 0;
const wire = (p) => ({
  id: p.id, status: 'current', title: p.title, spaceId: p.spaceId, parentId: p.parentId,
  version: version(p.v), body: { storage: { value: p.body, representation: 'storage' } },
  _links: { webui: `/spaces/PROC/pages/${p.id}` },
});
const listing = (results) => ({ results, _links: {} });

function confluence(method, path, query, body) {
  const wiki = `/ex/confluence/${CLOUD}/wiki`;
  if (path === '/oauth/token/accessible-resources') return [200, [{ id: CLOUD, url: SITE, name: 'acme', scopes: ['read:confluence-content.all', 'read:jira-work', 'write:jira-work', 'read:jira-user'] }]];
  if (!path.startsWith(wiki)) return null;
  const rest = path.slice(wiki.length);
  let m;
  if (rest === '/rest/api/search') {
    const hits = [
      { space: { key: 'PROC', name: 'Procurement' }, url: '/spaces/PROC' },
      ...[...pages.values()].map((p) => ({ content: { id: p.id, type: 'page', title: p.title }, resultGlobalContainer: { title: 'Procurement', displayUrl: '/spaces/PROC' }, url: `/spaces/PROC/pages/${p.id}` })),
    ];
    return [200, { results: hits, _links: {} }];
  }
  if (rest === '/rest/api/user') return [200, { accountId: 'acc-1', displayName: 'Sam Lee' }];
  if (rest === '/api/v2/spaces') return [200, listing([{ id: '65010', key: 'PROC', name: 'Procurement', _links: { webui: '/spaces/PROC' } }])];
  if ((m = /^\/api\/v2\/spaces\/([^/]+)\/permissions$/.exec(rest))) return [200, listing([{ principal: { type: 'group', id: 'g-1', name: 'Procurement team' }, operation: { key: 'read', targetType: 'space' } }])];
  if ((m = /^\/api\/v2\/spaces\/([^/]+)\/pages$/.exec(rest))) return [200, listing([...pages.values()].filter((p) => p.parentId === null).map(wire))];
  if ((m = /^\/rest\/api\/content\/([^/]+)\/restriction\/byOperation\/read$/.exec(rest))) return [200, { restrictions: { user: { results: [] }, group: { results: [{ name: 'Procurement team', id: 'g-1' }] } } }];
  if ((m = /^\/api\/v2\/pages\/([^/]+)\/children$/.exec(rest))) return [200, listing([...pages.values()].filter((p) => p.parentId === m[1]).map(wire))];
  if ((m = /^\/api\/v2\/pages\/([^/]+)\/attachments$/.exec(rest))) return [200, listing([])];
  if (rest === '/api/v2/pages' && method === 'POST') {
    const id = String(nextId++);
    const p = { id, title: body.title, spaceId: body.spaceId ?? '65010', parentId: body.parentId ?? '4000', v: 1, body: body.body?.value ?? '' };
    pages.set(id, p);
    savePages();
    log(`  created page ${id} "${p.title}"`);
    return [200, wire(p)];
  }
  if ((m = /^\/api\/v2\/pages\/([^/]+)$/.exec(rest))) {
    const p = pages.get(m[1]);
    if (!p) return [404, { errors: [{ title: 'Not found' }] }];
    if (method === 'GET') return [200, wire(p)];
    if (method === 'PUT') {
      if (failPuts > 0) { failPuts -= 1; log(`  refused update of page ${p.id}: unavailable`); return [503, { errors: [{ title: 'Service unavailable' }] }]; }
      if (body.version?.number !== p.v + 1) return [409, { errors: [{ title: `Version must be ${p.v + 1}` }] }];
      p.v += 1; p.title = body.title ?? p.title; p.body = body.body?.value ?? p.body; savePages();
      log(`  updated page ${p.id} to version ${p.v}: ${p.body.slice(0, 200)}`);
      return [200, wire(p)];
    }
    if (method === 'DELETE') { pages.delete(m[1]); savePages(); return [204, null]; }
  }
  return null;
}

// ---------------------------------------------------------------------------------------------------
// SharePoint (Microsoft Graph). Paths are `/graph/v1.0/<segments>`, built by the adapter's `Graph::url`.

const GRAPH = '/graph/v1.0';
const DRIVE = 'b!procdocs';
const SITE_ID = 'site-proc';
const SP_WEB = 'https://acme.sharepoint.com/sites/Procurement';
const LIB_WEB = `${SP_WEB}/Shared%20Documents`;
const ME = { user: { id: 'u-sam', displayName: 'Sam Lee' } };
const ROOT_ID = '01ROOT';

// A minimal Word file: a stored (uncompressed) zip of the three parts Word needs.
const CRC = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc32 = (b) => { let c = 0xffffffff; for (const x of b) c = CRC[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
function zip(files) {
  const locals = []; const centrals = []; let offset = 0;
  for (const [name, text] of files) {
    const n = Buffer.from(name); const d = Buffer.from(text); const crc = crc32(d);
    const l = Buffer.alloc(30); l.writeUInt32LE(0x04034b50, 0); l.writeUInt16LE(20, 4); l.writeUInt16LE(0x21, 12);
    l.writeUInt32LE(crc, 14); l.writeUInt32LE(d.length, 18); l.writeUInt32LE(d.length, 22); l.writeUInt16LE(n.length, 26);
    const c = Buffer.alloc(46); c.writeUInt32LE(0x02014b50, 0); c.writeUInt16LE(20, 4); c.writeUInt16LE(20, 6); c.writeUInt16LE(0x21, 14);
    c.writeUInt32LE(crc, 16); c.writeUInt32LE(d.length, 20); c.writeUInt32LE(d.length, 24); c.writeUInt16LE(n.length, 28); c.writeUInt32LE(offset, 42);
    locals.push(l, n, d); centrals.push(c, n); offset += 30 + n.length + d.length;
  }
  const dir = Buffer.concat(centrals); const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(files.length, 8); end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(dir.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, dir, end]);
}
const W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const para = (t) => `<w:p><w:r><w:t xml:space="preserve">${t}</w:t></w:r></w:p>`;
const kestrelDocx = zip([
  ['[Content_Types].xml', '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'],
  ['_rels/.rels', '<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'],
  ['word/document.xml', `<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="${W}"><w:body>${para('Kestrel Freight: terms of supply')}${para('Term: 24 months from 1 March 2026.')}${para('Rate: 1,180 GBP per container, reviewed each January.')}${para('Notice: 90 days in writing.')}</w:body></w:document>`],
]);

let seq = 1; // the delta change counter
let nextItem = 1;
const items = new Map();
const addItem = (it) => items.set(it.id, { v: 1, seq, deleted: false, at: '2026-01-29T09:00:00Z', ...it });
addItem({ id: ROOT_ID, name: 'root', parentId: null, folder: true });
addItem({ id: '01SUPPLIERS', name: 'Supplier contracts', parentId: ROOT_ID, folder: true });
addItem({ id: '01KESTREL', name: 'Kestrel terms.docx', parentId: '01SUPPLIERS', folder: false, v: 3, bytes: kestrelDocx, mime: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document' });
addItem({ id: '01NOTES', name: 'Notes.md', parentId: '01SUPPLIERS', folder: false, v: 2, bytes: Buffer.from('# Supplier notes\n\nKestrel Freight renews in March. Ask for the 2026 rate card.\n'), mime: 'text/markdown' });
const sessions = new Map();

const live = (id) => { const it = items.get(id); return it && !it.deleted ? it : undefined; };
const pathOf = (it) => (it.id === ROOT_ID ? '' : [pathOf(items.get(it.parentId)), it.name].filter(Boolean).join('/'));
const etag = (it) => `"{${it.id}},${it.v}"`;
const spErr = (status, code, message) => [status, { error: { code, message } }];
function spItem(it) {
  const out = {
    id: it.id, name: it.name, eTag: etag(it), cTag: `"c:{${it.id}},${it.v}"`,
    size: it.folder ? 0 : it.bytes.length, lastModifiedDateTime: it.at, lastModifiedBy: ME,
    parentReference: { driveId: DRIVE, driveType: 'documentLibrary', siteId: SITE_ID },
    webUrl: it.id === ROOT_ID ? LIB_WEB : `${LIB_WEB}/${pathOf(it).split('/').map(encodeURIComponent).join('/')}`,
  };
  if (it.parentId) { out.parentReference.id = it.parentId; out.parentReference.path = `/drives/${DRIVE}/root:${pathOf(items.get(it.parentId)) ? `/${pathOf(items.get(it.parentId))}` : ''}`; }
  if (it.folder) out.folder = { childCount: [...items.values()].filter((c) => c.parentId === it.id && !c.deleted).length };
  else out.file = { mimeType: it.mime ?? 'application/octet-stream' };
  if (it.deleted) out.deleted = { state: 'deleted' };
  return out;
}
const childOf = (parent, name) => [...items.values()].find((c) => !c.deleted && c.parentId === parent.id && c.name.toLowerCase() === name.toLowerCase());
const preview = (b) => b.toString('utf8').replace(/[^\x20-\x7e\n -￿]/g, '.').slice(0, 200);
function store(target, parent, name, bytes) {
  seq += 1;
  const at = new Date().toISOString().replace(/\.\d+Z$/, 'Z');
  if (target) {
    Object.assign(target, { v: target.v + 1, bytes, seq, at });
    log(`  updated item ${target.id} to version ${target.v}: ${preview(bytes)}`);
    return [200, spItem(target)];
  }
  const id = `01NEW${nextItem++}`;
  addItem({ id, name, parentId: parent.id, folder: false, bytes, mime: name.endsWith('.md') ? 'text/markdown' : 'application/octet-stream', seq, at });
  log(`  created item ${id} "${name}" in ${pathOf(parent) || 'the library root'}: ${preview(bytes)}`);
  return [201, spItem(items.get(id))];
}
function failedUpload(what) {
  if (failPuts <= 0) return null;
  failPuts -= 1;
  log(`  refused ${what}: unavailable`);
  return spErr(503, 'serviceNotAvailable', 'Service unavailable');
}
// The `If-Match` check of a write: 412 when the eTag is not the item's current one.
const stale = (it, headers) => headers['if-match'] && headers['if-match'] !== '*' && headers['if-match'] !== etag(it);

function graph(method, path, query, body, headers, buf) {
  if (path === '/microsoft/oauth2/v2.0/token' && method === 'POST') {
    return [200, { token_type: 'Bearer', access_token: 'microsoft-walk-token', expires_in: 3600, refresh_token: 'journeys-refresh',
      scope: 'https://graph.microsoft.com/Files.ReadWrite.All https://graph.microsoft.com/Sites.Read.All offline_access' }];
  }
  const host = `http://${headers.host ?? '127.0.0.1:9440'}`;
  let m;
  if ((m = /^\/graph\/__upload\/([^/]+)$/.exec(path)) && method === 'PUT') {
    const s = sessions.get(m[1]);
    if (!s) return spErr(404, 'itemNotFound', 'Upload session not found');
    const range = /bytes (\d+)-(\d+)\/(\d+)/.exec(headers['content-range'] ?? '');
    if (!range) return spErr(400, 'invalidRange', 'Content-Range missing');
    s.chunks.push(buf);
    const [end, total] = [Number(range[2]), Number(range[3])];
    if (end + 1 < total) return [202, { expirationDateTime: '2099-01-01T00:00:00Z', nextExpectedRanges: [`${end + 1}-`] }];
    sessions.delete(m[1]);
    return store(s.target && live(s.target.id), s.parent, s.name, Buffer.concat(s.chunks));
  }
  if (!path.startsWith(`${GRAPH}/`)) return null;
  let seg;
  try { seg = path.slice(GRAPH.length + 1).split('/').map(decodeURIComponent); } catch { return spErr(400, 'invalidRequest', 'Bad path'); }
  const site = { id: SITE_ID, displayName: 'Procurement', name: 'Procurement', webUrl: SP_WEB };
  const drive = { id: DRIVE, name: 'Documents', driveType: 'documentLibrary', webUrl: LIB_WEB };
  if (seg[0] === 'sites') {
    // Every search finds the one site, so a folder or library search can go on to look inside it.
    if (seg.length === 1) return [200, { value: [site] }];
    if (seg[1] !== SITE_ID) return spErr(404, 'itemNotFound', 'Site not found');
    if (seg.length === 2) return [200, site];
    if (seg.length === 3 && seg[2] === 'drives') return [200, { value: [drive] }];
    return null;
  }
  if (seg[0] === 'shares' && seg[2] === 'driveItem') {
    let wanted;
    try { wanted = decodeURI(Buffer.from(seg[1].replace(/^u!/, ''), 'base64url').toString('utf8')).split('?')[0].replace(/\/$/, ''); } catch { return spErr(400, 'invalidRequest', 'Bad share'); }
    const found = [...items.values()].find((it) => !it.deleted && decodeURI(spItem(it).webUrl) === wanted);
    return found ? [200, spItem(found)] : spErr(404, 'itemNotFound', 'No item at that address');
  }
  if (seg[0] !== 'drives') return null;
  if (seg[1] !== DRIVE) return spErr(404, 'itemNotFound', 'Drive not found');
  if (seg.length === 2) return [200, drive];

  // Which item the path names: `root`, `items/<id>`, or a path after `root:` / `items/<id>:`, ending at a
  // segment with a trailing `:` (then an action follows) or at the end (a lookup).
  let base; let i; let names = null;
  if (seg[2] === 'root' || seg[2] === 'root:') { base = live(ROOT_ID); i = 3; if (seg[2] === 'root:') names = []; }
  else if (seg[2] === 'items' && seg[3]) {
    const id = seg[3].endsWith(':') ? seg[3].slice(0, -1) : seg[3];
    if (seg[3].endsWith(':')) names = [];
    base = live(id); i = 4;
  } else return null;
  if (!base) return spErr(404, 'itemNotFound', 'Item not found');
  let target = base; let parent = null; let name = null;
  if (names) {
    while (i < seg.length) {
      const s = seg[i++];
      if (s.endsWith(':')) { names.push(s.slice(0, -1)); break; }
      names.push(s);
    }
    parent = base;
    for (const [k, n] of names.entries()) {
      const next = parent && childOf(parent, n);
      if (k === names.length - 1) { target = next; name = n; } else parent = next && next.folder ? next : null;
    }
  }
  const tail = seg.slice(i);
  const action = tail.join('/');

  if (action === '') {
    if (!target) return spErr(404, 'itemNotFound', 'Item not found');
    if (method === 'GET') return [200, spItem(target)];
    if (method === 'DELETE') {
      if (stale(target, headers)) return spErr(412, 'resourceModified', 'ETag does not match');
      seq += 1; Object.assign(target, { deleted: true, seq });
      log(`  deleted item ${target.id} "${target.name}"`);
      return [204, null];
    }
  }
  if (action.startsWith("search(q='") && method === 'GET') {
    const q = action.slice(10, -2).replace(/''/g, "'").toLowerCase();
    const under = (it) => it && (it.id === base.id || under(items.get(it.parentId)));
    const hits = [...items.values()].filter((it) => !it.deleted && it.id !== ROOT_ID && under(items.get(it.parentId)) && it.name.toLowerCase().includes(q));
    return [200, { value: hits.map(spItem) }];
  }
  if (!target) {
    if (action === 'content' && method === 'PUT' && parent) return failedUpload(`upload of new file "${name}"`) ?? store(null, parent, name, buf);
    if (action === 'createUploadSession' && method === 'POST' && parent) {
      const stop = failedUpload(`upload session for new file "${name}"`); if (stop) return stop;
      const id = `s${nextItem++}`; sessions.set(id, { target: null, parent, name, chunks: [] });
      return [200, { uploadUrl: `${host}/graph/__upload/${id}`, expirationDateTime: '2099-01-01T00:00:00Z' }];
    }
    return spErr(404, 'itemNotFound', 'Item not found');
  }
  if (action === 'children') {
    if (!target.folder) return spErr(400, 'invalidRequest', 'Not a folder');
    if (method === 'GET') return [200, { value: [...items.values()].filter((c) => c.parentId === target.id && !c.deleted).map(spItem) }];
    if (method === 'POST') {
      if (!body.name) return spErr(400, 'invalidRequest', 'Name missing');
      if (childOf(target, body.name)) return spErr(409, 'nameAlreadyExists', 'Name already exists');
      seq += 1; const id = `01NEW${nextItem++}`;
      addItem({ id, name: body.name, parentId: target.id, folder: true, seq });
      log(`  created folder ${id} "${body.name}"`);
      return [201, spItem(items.get(id))];
    }
  }
  if (action === 'content') {
    if (method === 'GET') return target.folder ? spErr(400, 'invalidRequest', 'Not a file') : [200, target.bytes, target.mime];
    if (method === 'PUT') {
      if (names && query.get('@microsoft.graph.conflictBehavior') === 'fail') return spErr(409, 'nameAlreadyExists', 'Name already exists');
      if (stale(target, headers)) { log(`  refused update of item ${target.id}: eTag ${headers['if-match']} is not ${etag(target)}`); return spErr(412, 'resourceModified', 'ETag does not match'); }
      return failedUpload(`upload of item ${target.id}`) ?? store(target, null, null, buf);
    }
  }
  if (action === 'createUploadSession' && method === 'POST') {
    if (names && body.item?.['@microsoft.graph.conflictBehavior'] === 'fail') return spErr(409, 'nameAlreadyExists', 'Name already exists');
    if (stale(target, headers)) return spErr(412, 'resourceModified', 'ETag does not match');
    const stop = failedUpload(`upload session for item ${target.id}`); if (stop) return stop;
    const id = `s${nextItem++}`; sessions.set(id, { target, parent: null, name: null, chunks: [] });
    return [200, { uploadUrl: `${host}/graph/__upload/${id}`, expirationDateTime: '2099-01-01T00:00:00Z' }];
  }
  if (action === 'versions' && method === 'GET') {
    const versions = Array.from({ length: target.v }, (_, k) => ({ id: `${target.v - k}.0`, lastModifiedDateTime: target.at, lastModifiedBy: ME }));
    return [200, { value: versions }];
  }
  if (action === 'permissions' && method === 'GET') {
    return [200, { value: [{ id: 'p-1', roles: ['write'], grantedToV2: { group: { id: 'g-proc', displayName: 'Procurement team' } } }] }];
  }
  if (action === 'delta' && method === 'GET') {
    const token = query.get('token');
    const since = token === 'latest' ? seq : Number(token ?? 0);
    const changed = token === 'latest' ? [] : [...items.values()].filter((it) => it.seq > since);
    return [200, { value: changed.map(spItem), '@odata.deltaLink': `${host}${GRAPH}/drives/${encodeURIComponent(DRIVE)}/root/delta?token=${seq}` }];
  }
  return null;
}

// ---------------------------------------------------------------------------------------------------
// Jira: REST API v3 under `/rest/api/3` (API token) or `/ex/jira/<cloud>/rest/api/3` (OAuth).

const JIRA_ME = { accountId: 'acc-1', displayName: 'Sam Lee', emailAddress: 'sam@acme.example', accountType: 'atlassian', active: true };
const JIRA_PROJECT = { id: '10000', key: 'PROC', name: 'Procurement' };
const QUOTE = Buffer.from('Kestrel Freight quote, January 2026\n24-month renewal: 1,180 GBP per container.\nValid until 28 February 2026.\n');
const adfText = (text) => ({ version: 1, type: 'doc', content: text.split('\n\n').map((p) => ({ type: 'paragraph', content: [{ type: 'text', text: p }] })) });
const textOfAdf = (n) => (n?.type === 'text' ? n.text : (n?.content ?? []).map(textOfAdf).join(n?.type === 'doc' ? '\n' : ''));
const issues = new Map([
  ['PROC-12', {
    id: '10012', key: 'PROC-12', summary: 'Renew Kestrel contract', labels: [], assignee: JIRA_ME,
    status: { name: 'In Progress', statusCategory: { key: 'indeterminate', name: 'In Progress' } },
    updated: '2026-01-29T09:00:00.000+0000',
    description: adfText('The Kestrel Freight contract ends on 28 February 2026.\n\nCompare the attached quote with the current terms and draft the renewal.'),
    attachment: [{ id: '20001', filename: 'quote.txt', mimeType: 'text/plain', size: QUOTE.length, created: '2026-01-28T16:20:00.000+0000', bytes: QUOTE }],
    comments: [],
  }],
]);
let nextIssue = 13;
let nextComment = 30001;
const jiraNow = () => new Date().toISOString().replace('Z', '+0000');
const jiraErr = (status, message) => [status, { errorMessages: [message], errors: {} }];
function jiraIssue(is) {
  return {
    id: is.id, key: is.key, self: `${SITE}/rest/api/3/issue/${is.id}`,
    fields: {
      summary: is.summary, description: is.description, assignee: is.assignee, status: is.status, labels: is.labels,
      updated: is.updated, project: JIRA_PROJECT, issuetype: { id: '10001', name: 'Task', subtask: false },
      attachment: is.attachment.map(({ bytes, ...a }) => a), subtasks: [], issuelinks: [],
    },
  };
}

function jira(method, path, query, body) {
  let rest;
  let m;
  if (path.startsWith('/rest/api/3/')) rest = path.slice('/rest/api/3'.length);
  else if ((m = /^\/ex\/jira\/[^/]+\/rest\/api\/3(\/.*)$/.exec(path))) rest = m[1];
  else return null;
  if (rest === '/myself' && method === 'GET') return [200, JIRA_ME];
  if (rest === '/project/PROC' && method === 'GET') return [200, JIRA_PROJECT];
  if (rest === '/user/assignable/search' && method === 'GET') {
    const id = query.get('accountId');
    return [200, id && id !== JIRA_ME.accountId ? [] : [JIRA_ME]];
  }
  if (rest === '/issue/createmeta/PROC/issuetypes' && method === 'GET') {
    return [200, { issueTypes: [{ id: '10001', name: 'Task', subtask: false }, { id: '10003', name: 'Sub-task', subtask: true }], maxResults: 50, startAt: 0, total: 2 }];
  }
  if (rest === '/search/jql' && method === 'POST') {
    const jql = String(body.jql ?? '');
    const text = /text ~ "((?:[^"\\]|\\.)*)"/.exec(jql)?.[1]?.toLowerCase();
    const label = /labels = "((?:[^"\\]|\\.)*)"/.exec(jql)?.[1];
    const keys = /key in \(([^)]*)\)/.exec(jql)?.[1]?.split(',').map((k) => k.trim()) ?? [];
    const found = [...issues.values()].filter((is) => {
      if (text) return `${is.summary}\n${textOfAdf(is.description)}`.toLowerCase().includes(text);
      if (label) return is.labels.includes(label) || keys.includes(is.key);
      return true;
    });
    return [200, { issues: found.map(jiraIssue), isLast: true }];
  }
  if (rest === '/issue' && method === 'POST') {
    const key = `PROC-${nextIssue++}`;
    const summary = body.fields?.summary ?? '';
    issues.set(key, { id: String(10000 + nextIssue - 1), key, summary, labels: [], assignee: null,
      status: { name: 'To Do', statusCategory: { key: 'new', name: 'To Do' } }, updated: jiraNow(), description: null, attachment: [], comments: [] });
    log(`  created issue ${key} "${summary}"`);
    return [201, { id: issues.get(key).id, key, self: `${SITE}/rest/api/3/issue/${key}` }];
  }
  if ((m = /^\/attachment\/content\/([^/]+)$/.exec(rest)) && method === 'GET') {
    const a = [...issues.values()].flatMap((is) => is.attachment).find((x) => x.id === m[1]);
    return a ? [200, a.bytes, a.mimeType] : jiraErr(404, 'Attachment not found');
  }
  if ((m = /^\/issue\/([^/]+)(\/comment|\/assignee)?$/.exec(rest))) {
    const is = issues.get(decodeURIComponent(m[1]).toUpperCase());
    if (!is) return jiraErr(404, 'Issue does not exist or you do not have permission to see it.');
    if (!m[2] && method === 'GET') return [200, jiraIssue(is)];
    if (!m[2] && method === 'PUT') {
      for (const op of body.update?.labels ?? []) if (op.add && !is.labels.includes(op.add)) is.labels.push(op.add);
      is.updated = jiraNow();
      log(`  labelled ${is.key}: ${is.labels.join(', ')}`);
      return [204, null];
    }
    if (m[2] === '/assignee' && method === 'PUT') {
      is.assignee = body.accountId === JIRA_ME.accountId ? JIRA_ME : { accountId: body.accountId, displayName: body.accountId, accountType: 'atlassian', active: true };
      is.updated = jiraNow();
      log(`  assigned ${is.key} to ${body.accountId}`);
      return [204, null];
    }
    if (m[2] === '/comment' && method === 'GET') {
      const start = Number(query.get('startAt') ?? 0); const max = Number(query.get('maxResults') ?? 100);
      return [200, { startAt: start, maxResults: max, total: is.comments.length, comments: is.comments.slice(start, start + max) }];
    }
    if (m[2] === '/comment' && method === 'POST') {
      const at = jiraNow();
      const c = { id: String(nextComment++), author: JIRA_ME, updateAuthor: JIRA_ME, body: body.body, created: at, updated: at };
      is.comments.push(c); is.updated = at;
      log(`  commented on ${is.key} (comment ${c.id}): ${textOfAdf(body.body).slice(0, 200)}`);
      return [201, c];
    }
  }
  return null;
}

http
  .createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const buf = Buffer.concat(chunks);
      const raw = buf.toString('utf8');
      const url = new URL(req.url, 'http://127.0.0.1');
      let body = {};
      try { body = raw ? JSON.parse(raw) : {}; } catch { /* not JSON */ }
      if (url.pathname === '/__fail_puts') {
        failPuts = Number(url.searchParams.get('n') ?? '1');
        log(`next ${failPuts} page updates or SharePoint uploads fail`);
        res.writeHead(200); return res.end('ok');
      }
      const args = [req.method, url.pathname, url.searchParams, body, req.headers, buf];
      const out = confluence(...args) ?? graph(...args) ?? jira(...args) ?? [404, { error: 'stand-in has no answer' }];
      log(`${req.method} ${url.pathname}${url.search} → ${out[0]}${buf.length ? ` body ${buf.length}b` : ''}`);
      if (Buffer.isBuffer(out[1])) {
        res.writeHead(out[0], { 'content-type': out[2] ?? 'application/octet-stream', 'content-length': out[1].length });
        return res.end(out[1]);
      }
      res.writeHead(out[0], { 'content-type': 'application/json' });
      res.end(out[1] === null ? '' : JSON.stringify(out[1]));
    });
  })
  .listen(9440, '127.0.0.1', () => log('listening on 127.0.0.1:9440'));

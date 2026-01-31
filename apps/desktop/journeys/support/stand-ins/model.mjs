// A scripted stand-in for the model Claude Code talks to, on 127.0.0.1:9339, for the journeys. Nothing
// leaves the computer and nothing is spent. It runs in the folder STAND_IN_DIR (the run folder), where it
// reads script.json and writes model.log.
//
// script.json (read again on every request, so a journey step can change it):
//   { "rules": [ { "who": "ordin" | "agent", "when": "<text in the person's latest message or the agent's brief>",
//                  "steps": [ { "tool": "<name>", "input": { ... } }, ..., { "text": "<the reply>" } ] } ],
//     "default_ordin": "<Ordin's reply when no rule matches>", "default_agent": "<an agent's reply>" }
// Ordin's requests carry its own tools (mcp__ordin__*); an agent's do not. A rule's tool steps are made one
// per model turn: a step is done once its tool_use_id is in the request (Claude Code sent its result back).
// A tool name without a prefix is Ordin's (`find_agent` → `mcp__ordin__find_agent`).
//
// Placeholders in a step's input:
//   {{key:<agent name>}}  the agent's key from the directory Ordin was given;
//   {{item:<part of a path>}}  the newest working-copy item id Ordin's turn line (OH-WC-1) named for that path;
//   {{file:<file name>}}  the newest file of that name under STAND_IN_FIND (folders separated by `;`), such
//                         as a linked copy, whose folder has an id only the program knows;
//   {{dir:<file name>}}   the folder that file is in.
import { createHash } from 'node:crypto';
import http from 'node:http';
import { appendFileSync, existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

process.chdir(process.env.STAND_IN_DIR ?? '.');
const log = (line) => appendFileSync('model.log', `${new Date().toISOString()} ${line}\n`);
const script = () => (existsSync('script.json') ? JSON.parse(readFileSync('script.json', 'utf8')) : { rules: [] });

/** The newest file named `name` under the folders of STAND_IN_FIND. */
function newestFile(name) {
  let best = null;
  const walk = (dir, depth) => {
    if (depth > 6 || !existsSync(dir)) return;
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path, depth + 1);
      else if (entry.name === name) {
        const at = statSync(path).mtimeMs;
        if (best === null || at > best.at) best = { path, at };
      }
    }
  };
  for (const root of (process.env.STAND_IN_FIND ?? '').split(';').filter(Boolean)) walk(root, 0);
  return best?.path ?? `no-file-named-${name}`;
}

/** The messages from the last user message that is not only tool results onwards, as JSON: a scripted
 * step counts as done only when its tool result is in this part, so a person asking again reruns it. */
function sinceLastAsk(body) {
  return lastAsk(body).tail;
}

/** Where the person's latest ask is (`marker`, used in tool ids so each ask has its own) and what came after it. */
function lastAsk(body) {
  const messages = body.messages ?? [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role !== 'user') continue;
    if (typeof m.content === 'string') return { marker: `${i}`, tail: JSON.stringify(messages.slice(i + 1)) };
    // Claude Code merges a tool result and the person's next words into one message: only the blocks
    // after the last text block, and the messages after it, belong to the person's latest ask.
    const blocks = m.content ?? [];
    let last = -1;
    blocks.forEach((b, k) => { if (b.type === 'text') last = k; });
    if (last >= 0) return { marker: `${i}x${last}`, tail: JSON.stringify([...blocks.slice(last + 1), ...messages.slice(i + 1)]) };
  }
  return { marker: '0', tail: JSON.stringify(messages) };
}

/** The text of the last user message that is not only tool results. */
function lastUserText(body) {
  const messages = body.messages ?? [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role !== 'user') continue;
    if (typeof m.content === 'string') return m.content;
    const texts = (m.content ?? []).filter((b) => b.type === 'text').map((b) => b.text);
    // Claude Code merges asks into one message; the newest is the last text block.
    if (texts.length > 0) return texts[texts.length - 1];
  }
  return '';
}

function sse(res, content, stopReason, model) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  const send = (data) => res.write(`event: ${data.type}\ndata: ${JSON.stringify(data)}\n\n`);
  send({ type: 'message_start', message: { id: 'msg_standin', type: 'message', role: 'assistant', model, content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 1 } } });
  content.forEach((block, index) => {
    if (block.text !== undefined) {
      send({ type: 'content_block_start', index, content_block: { type: 'text', text: '' } });
      send({ type: 'content_block_delta', index, delta: { type: 'text_delta', text: block.text } });
    } else {
      send({ type: 'content_block_start', index, content_block: { type: 'tool_use', id: block.id, name: block.name, input: {} } });
      send({ type: 'content_block_delta', index, delta: { type: 'input_json_delta', partial_json: JSON.stringify(block.input ?? {}) } });
    }
    send({ type: 'content_block_stop', index });
  });
  send({ type: 'message_delta', delta: { stop_reason: stopReason, stop_sequence: null }, usage: { output_tokens: 5 } });
  send({ type: 'message_stop' });
  res.end();
}

function reply(raw, body) {
  const s = script();
  const isOrdin = raw.includes('mcp__ordin__');
  const who = isOrdin ? 'ordin' : 'agent';
  const said = lastUserText(body);
  const rules = (s.rules ?? []).map((r, n) => ({ ...r, n })).filter((r) => (r.who ?? 'ordin') === who && said.includes(r.when));
  const rule = rules[rules.length - 1];
  if (rule !== undefined) {
    // Claude Code merges turns and reorders the blocks of a merged message, so progress is counted, not
    // read from the last block: each ask of the person for this scene starts it once (step 0); after that
    // the newest call of the scene (ids carry the time they were made) moves it on once its result is back.
    // A scene's calls carry a tag made from the rule itself, not its place in the script: the journeys share
    // a conversation, and each journey's script numbers its rules from 0.
    const tag = createHash('sha256').update(JSON.stringify([rule.who ?? 'ordin', rule.when, rule.steps])).digest('hex').slice(0, 12);
    const messages = body.messages ?? [];
    const blocks = messages.flatMap((m) => (Array.isArray(m.content) ? m.content.map((b) => ({ role: m.role, b })) : [{ role: m.role, b: { type: 'text', text: String(m.content) } }]));
    // Every ask of the person that holds `when` counts, wherever Claude Code put it: it merges the person's
    // words into the message that carries the tool results, after them. The journeys' rules each have
    // their own `when`.
    const asks = blocks.filter(({ role, b }) => role === 'user' && b.type === 'text' && String(b.text).includes(rule.when)).length;
    const calls = blocks
      .filter(({ role, b }) => role === 'assistant' && b.type === 'tool_use')
      .map(({ b }) => ({ id: b.id, x: /^toolu_standin_([0-9a-f]+)_(\d+)_(\d+)$/.exec(b.id) }))
      .filter((c) => c.x !== null && c.x[1] === tag);
    const starts = calls.filter((c) => Number(c.x[2]) === 0).length;
    let next = 0;
    if (calls.length > 0 && starts >= asks) {
      const newest = calls.reduce((a, c) => (Number(c.x[3]) > Number(a.x[3]) ? c : a));
      next = Number(newest.x[2]) + (raw.includes(`"tool_use_id":"${newest.id}"`) ? 1 : 0);
    }
    const step = rule.steps[next];
    if (step === undefined) return { content: [{ text: 'Done.' }], stop: 'end_turn', note: `rule ${rule.n} done` };
    if (step.text !== undefined) return { content: [{ text: step.text }], stop: 'end_turn', note: `rule ${rule.n} text`, delay: step.delay ?? 0 };
    const id = `toolu_standin_${tag}_${next}_${Date.now()}`;
    const name = step.tool.includes('__') || /^[A-Z]/.test(step.tool) ? step.tool : `mcp__ordin__${step.tool}`;
    // `{{key:<agent name>}}` in a step's input is the agent's key from the directory Ordin was given.
    const filled = JSON.parse(JSON.stringify(step.input ?? {}).replace(/\{\{key:([^}]+)\}\}/g, (_, agentName) => {
      const m = new RegExp(`- ${agentName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\(key ([0-9a-f-]{36})\\)`).exec(raw.replace(/\\n/g, '\n'));
      return m ? m[1] : 'unknown-key';
    }).replace(/\{\{item:([^}]+)\}\}/g, (_, path) => {
      // `{{item:<part of the origin path>}}`: the newest working-copy item id the turn line OH-WC-1 named for that path.
      const re = new RegExp(`- [^\\n]*?${path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[^\\n]*? \\(item ([0-9a-f-]{36})\\)`, 'g');
      const all = [...raw.replace(/\\n/g, '\n').matchAll(re)];
      return all.length ? all[all.length - 1][1] : 'unknown-item';
    }).replace(/\{\{file:([^}]+)\}\}/g, (_, name) => JSON.stringify(newestFile(name)).slice(1, -1))
      .replace(/\{\{dir:([^}]+)\}\}/g, (_, name) => JSON.stringify(dirname(newestFile(name))).slice(1, -1)));
    return { content: [{ id, name, input: filled }], stop: 'tool_use', note: `rule ${rule.n} ${name}` };
  }
  const text = isOrdin ? (s.default_ordin ?? 'This is the stand-in model on the test machine. I read your message.') : (s.default_agent ?? 'Done.');
  return { content: [{ text }], stop: 'end_turn', note: `${who} default` };
}

http
  .createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
      if (req.url.startsWith('/v1/messages/count_tokens')) {
        res.writeHead(200, { 'content-type': 'application/json' });
        return res.end(JSON.stringify({ input_tokens: 10 }));
      }
      if (!req.url.startsWith('/v1/messages')) {
        res.writeHead(200, { 'content-type': 'application/json' });
        return res.end('{}');
      }
      let body = {};
      try { body = JSON.parse(raw); } catch { /* not JSON */ }
      const out = reply(raw, body);
      if (out.delay) log(`waiting ${out.delay} ms before answering`);
      setTimeout(() => answer(out, body), out.delay ?? 0);
    });
    const answer = (out, body) => {
      try { writeFileSync(`model-last-request-${raw.includes('mcp__ordin__') ? 'ordin' : 'agent'}.json`, raw); } catch { /* best effort */ }
      log(`POST ${req.url} ${raw.length}b → ${out.note}`);
      // OH-WC-1: what the latest ask said about changed linked documents.
      const said = lastUserText(body).split('\n').filter((l) => l.startsWith('Linked documents changed') || / \(item [0-9a-f-]{36}\)$/.test(l));
      if (said.length) log(`  turn says: ${said.join(' | ')}`);
      const model = body.model ?? 'claude-standin';
      if (!body.stream) {
        res.writeHead(200, { 'content-type': 'application/json' });
        const content = out.content.map((b) => (b.text !== undefined ? { type: 'text', text: b.text } : { type: 'tool_use', id: b.id, name: b.name, input: b.input }));
        return res.end(JSON.stringify({ id: 'msg_standin', type: 'message', role: 'assistant', model, content, stop_reason: out.stop, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 12 } }));
      }
      sse(res, out.content, out.stop, model);
    };
  })
  .listen(9339, '127.0.0.1', () => log('listening on 127.0.0.1:9339 (scripted)'));

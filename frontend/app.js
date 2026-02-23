// ── RustedClaw Frontend Application ─────────────────────────────────────────
// Vanilla JS — no build step, connects to /v1/* API endpoints
// Pages: Dashboard, Chat, Memory, Tools, Contracts, Usage, Channels,
//        Routines, Jobs, Logs, Settings

(function () {
    'use strict';

    // ── State ─────────────────────────────────────────────────────────────

    const state = {
        currentPage: 'dashboard',
        conversationId: null,
        ws: null,
        logSource: null,
        isStreaming: false,
        logFilter: '',
    };

    // ── API Helpers ───────────────────────────────────────────────────────

    const api = {
        async get(path) {
            const res = await fetch(`/v1${path}`);
            if (!res.ok) throw new Error(`API ${res.status}: ${res.statusText}`);
            return res.json();
        },

        async post(path, body) {
            const res = await fetch(`/v1${path}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            if (!res.ok) {
                const txt = await res.text().catch(() => res.statusText);
                throw new Error(`API ${res.status}: ${txt}`);
            }
            return res.json();
        },

        async patch(path, body) {
            const res = await fetch(`/v1${path}`, {
                method: 'PATCH',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            if (!res.ok) throw new Error(`API ${res.status}`);
            return res.json();
        },

        async del(path) {
            const res = await fetch(`/v1${path}`, { method: 'DELETE' });
            if (!res.ok) throw new Error(`API ${res.status}`);
            return res.json();
        },

        streamChat(body) {
            return fetch(`/v1/chat/stream`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
        },
    };

    // ── Navigation ────────────────────────────────────────────────────────

    function navigateTo(page) {
        state.currentPage = page;

        document.querySelectorAll('.nav-link').forEach(el => {
            el.classList.toggle('active', el.dataset.page === page);
        });

        document.querySelectorAll('.page').forEach(el => {
            el.classList.toggle('active', el.id === `page-${page}`);
        });

        switch (page) {
            case 'dashboard': loadDashboard(); break;
            case 'tools':     loadTools(); break;
            case 'memory':    loadMemory(); break;
            case 'contracts': loadContracts(); break;
            case 'usage':     loadUsage(); break;
            case 'channels':  loadChannels(); break;
            case 'routines':  loadRoutines(); break;
            case 'jobs':      loadJobs(); break;
            case 'logs':      connectLogStream(); break;
            case 'settings':  loadSettings(); break;
        }
    }

    // ── Toast ─────────────────────────────────────────────────────────────

    function toast(message, type = 'info') {
        const container = document.getElementById('toast-container');
        const el = document.createElement('div');
        el.className = `toast ${type}`;
        el.textContent = message;
        container.appendChild(el);
        setTimeout(() => el.remove(), 4000);
    }

    // ── Utilities ─────────────────────────────────────────────────────────

    function escapeHtml(str) {
        if (typeof str !== 'string') return '';
        const div = document.createElement('div');
        div.textContent = str;
        return div.innerHTML;
    }

    function formatUptime(secs) {
        const d = Math.floor(secs / 86400);
        const h = Math.floor((secs % 86400) / 3600);
        const m = Math.floor((secs % 3600) / 60);
        const s = Math.floor(secs % 60);
        if (d > 0) return `${d}d ${h}h ${m}m`;
        if (h > 0) return `${h}h ${m}m ${s}s`;
        if (m > 0) return `${m}m ${s}s`;
        return `${s}s`;
    }

    function formatCost(val) {
        if (typeof val !== 'number') return '$0.00';
        return val < 0.01 && val > 0 ? `$${val.toFixed(6)}` : `$${val.toFixed(4)}`;
    }

    function formatNumber(n) {
        if (typeof n !== 'number') return '0';
        if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
        if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
        return n.toLocaleString();
    }

    function channelIcon(name) {
        const icons = {
            web: '🌐', telegram: '✈️', discord: '🎮', slack: '💼',
            whatsapp: '📱', imessage: '💬', matrix: '🔲', webhook: '🔗',
            cli: '⌨️',
        };
        return icons[(name || '').toLowerCase()] || '📡';
    }

    // ── Dashboard ─────────────────────────────────────────────────────────

    async function loadDashboard() {
        try {
            const [status, tools, channels, contracts, usage] = await Promise.all([
                api.get('/status'),
                api.get('/tools').catch(() => ({ tools: [] })),
                api.get('/channels').catch(() => ({ channels: [] })),
                api.get('/contracts').catch(() => []),
                api.get('/usage').catch(() => ({})),
            ]);

            // Stats
            const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
            set('dash-status', status.status || 'unknown');
            set('dash-uptime', formatUptime(status.uptime_secs || 0));
            set('dash-conversations', status.active_conversations || 0);
            set('dash-tools', status.tools_count || 0);
            set('dash-contracts', status.contracts_count || 0);
            set('dash-cost', formatCost(status.session_cost_usd));
            set('dash-traces', status.trace_count || 0);
            set('dash-memory', status.memory_entries || 0);
            set('dash-provider', status.provider || '—');

            // Tools compact list
            const toolsList = document.getElementById('dash-tools-list');
            const tArr = tools.tools || [];
            if (tArr.length === 0) {
                toolsList.innerHTML = '<span style="color:var(--text-dim)">No tools</span>';
            } else {
                toolsList.innerHTML = tArr.map(t => `
                    <div class="compact-item">
                        <span class="ci-name">🔧 ${escapeHtml(t.name)}</span>
                        <span class="ci-desc">${escapeHtml(t.description || '')}</span>
                    </div>
                `).join('');
            }

            // Channels compact list
            const chList = document.getElementById('dash-channels-list');
            const chs = channels.channels || [];
            if (chs.length === 0) {
                chList.innerHTML = '<span style="color:var(--text-dim)">No channels</span>';
            } else {
                chList.innerHTML = chs.map(ch => {
                    const name = ch.name || ch;
                    const healthy = ch.health === 'healthy';
                    return `
                        <div class="compact-item">
                            <span class="ci-name">${channelIcon(name)} ${escapeHtml(name)}</span>
                            <span class="badge ${healthy ? 'badge-green' : 'badge-red'}">${healthy ? 'Healthy' : 'Down'}</span>
                        </div>
                    `;
                }).join('');
            }

            // Contracts compact list
            const ctList = document.getElementById('dash-contracts-list');
            const cts = Array.isArray(contracts) ? contracts : [];
            if (cts.length === 0) {
                ctList.innerHTML = '<span style="color:var(--text-dim)">No contracts — agent is unrestricted</span>';
            } else {
                ctList.innerHTML = cts.slice(0, 5).map(c => `
                    <div class="compact-item">
                        <span class="ci-name">🛡️ ${escapeHtml(c.name)}</span>
                        <span class="badge badge-blue">${escapeHtml(c.enforcement || c.action || 'active')}</span>
                    </div>
                `).join('');
            }

            updateStatus(true);
        } catch (e) {
            toast(`Dashboard: ${e.message}`, 'error');
        }
    }

    // ── Chat ──────────────────────────────────────────────────────────────

    function addChatMessage(role, content) {
        const container = document.getElementById('chat-messages');
        const empty = container.querySelector('.empty-state');
        if (empty) empty.remove();

        const el = document.createElement('div');
        el.className = `chat-message ${role}`;
        el.textContent = content;
        container.appendChild(el);
        container.scrollTop = container.scrollHeight;
        return el;
    }

    function addToolCard(name, input, output, success) {
        const container = document.getElementById('chat-messages');
        const el = document.createElement('div');
        el.className = 'chat-message assistant';
        el.innerHTML = `
            <span class="tool-badge">🔧 ${escapeHtml(name)}</span>
            <span class="badge ${success ? 'badge-green' : 'badge-red'}">${success ? 'OK' : 'Failed'}</span>
            <div class="tool-card">
                <div><strong>Input:</strong> ${escapeHtml(typeof input === 'string' ? input : JSON.stringify(input))}</div>
                <div><strong>Output:</strong> ${escapeHtml(typeof output === 'string' ? output : JSON.stringify(output))}</div>
            </div>
        `;
        container.appendChild(el);
        container.scrollTop = container.scrollHeight;
    }

    async function sendMessage() {
        const input = document.getElementById('chat-input');
        const message = input.value.trim();
        if (!message || state.isStreaming) return;

        input.value = '';
        addChatMessage('user', message);

        state.isStreaming = true;
        document.getElementById('send-btn').disabled = true;

        try {
            const body = {
                message,
                pattern: 'react',
                conversation_id: state.conversationId || undefined,
            };

            const response = await api.streamChat(body);
            if (!response.ok) throw new Error(`Stream error: ${response.status}`);

            const reader = response.body.getReader();
            const decoder = new TextDecoder();
            let assistantEl = null;
            let fullText = '';
            let buffer = '';

            while (true) {
                const { done, value } = await reader.read();
                if (done) break;

                buffer += decoder.decode(value, { stream: true });
                const events = parseSSE(buffer);
                buffer = events.remainder;

                for (const event of events.parsed) {
                    switch (event.type) {
                        case 'chunk': {
                            const data = JSON.parse(event.data);
                            if (!assistantEl) {
                                assistantEl = addChatMessage('assistant', '');
                                assistantEl.classList.add('streaming-cursor');
                            }
                            fullText += data.content;
                            assistantEl.textContent = fullText;
                            document.getElementById('chat-messages').scrollTop =
                                document.getElementById('chat-messages').scrollHeight;
                            break;
                        }
                        case 'tool_call': {
                            const data = JSON.parse(event.data);
                            addChatMessage('system', `⚙️ Calling tool: ${data.name}`);
                            break;
                        }
                        case 'tool_result': {
                            const data = JSON.parse(event.data);
                            addToolCard(data.name, data.input || '', data.output, data.success);
                            break;
                        }
                        case 'done': {
                            const data = JSON.parse(event.data);
                            if (data.conversation_id) state.conversationId = data.conversation_id;
                            if (assistantEl) assistantEl.classList.remove('streaming-cursor');
                            break;
                        }
                        case 'error': {
                            const data = JSON.parse(event.data);
                            toast(data.message, 'error');
                            break;
                        }
                    }
                }
            }

            if (assistantEl) assistantEl.classList.remove('streaming-cursor');
        } catch (e) {
            toast(`Error: ${e.message}`, 'error');
        } finally {
            state.isStreaming = false;
            document.getElementById('send-btn').disabled = false;
        }
    }

    function parseSSE(text) {
        const lines = text.split('\n');
        const parsed = [];
        let currentEvent = null;
        let remainder = '';

        for (let i = 0; i < lines.length; i++) {
            const line = lines[i];
            if (line.startsWith('event: ')) {
                currentEvent = { type: line.slice(7).trim(), data: '' };
            } else if (line.startsWith('data: ') && currentEvent) {
                currentEvent.data = line.slice(6);
            } else if (line === '' && currentEvent) {
                parsed.push(currentEvent);
                currentEvent = null;
            }
        }

        if (currentEvent) {
            const lastNewline = text.lastIndexOf('\n\n');
            remainder = lastNewline >= 0 ? text.slice(lastNewline + 2) : text;
        }

        return { parsed, remainder };
    }

    // ── Tools ─────────────────────────────────────────────────────────────

    async function loadTools() {
        try {
            const data = await api.get('/tools');
            const container = document.getElementById('tools-list');
            const tools = data.tools || [];

            document.getElementById('tools-count-badge').textContent = `${tools.length} tools`;

            if (tools.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No tools registered</p></div>';
                return;
            }

            container.innerHTML = tools.map(tool => {
                const params = tool.parameters || tool.input_schema || {};
                const props = params.properties || {};
                const required = params.required || [];

                let paramHtml = '';
                if (Object.keys(props).length > 0) {
                    paramHtml = `
                        <table class="param-table">
                            <tr><th>Parameter</th><th>Type</th><th>Required</th><th>Description</th></tr>
                            ${Object.entries(props).map(([k, v]) => `
                                <tr>
                                    <td>${escapeHtml(k)}</td>
                                    <td>${escapeHtml(v.type || 'any')}</td>
                                    <td>${required.includes(k) ? '✓' : ''}</td>
                                    <td style="color:var(--text-muted)">${escapeHtml(v.description || '')}</td>
                                </tr>
                            `).join('')}
                        </table>
                    `;
                }

                return `
                    <div class="card">
                        <div class="card-header">
                            <span class="card-title">🔧 ${escapeHtml(tool.name)}</span>
                            <span class="badge badge-green">Active</span>
                        </div>
                        <div class="card-body">${escapeHtml(tool.description)}</div>
                        ${paramHtml}
                    </div>
                `;
            }).join('');
        } catch (e) {
            toast(`Failed to load tools: ${e.message}`, 'error');
        }
    }

    // ── Memory ────────────────────────────────────────────────────────────

    async function loadMemory(query) {
        try {
            const params = query ? `?query=${encodeURIComponent(query)}` : '';
            const data = await api.get(`/memory${params}`);
            const container = document.getElementById('memory-list');
            const memories = data.memories || [];
            if (memories.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No memories found</p></div>';
                return;
            }
            container.innerHTML = memories.map(mem => `
                <div class="card" data-id="${escapeHtml(mem.id)}">
                    <div class="card-header">
                        <span class="card-title">${escapeHtml(mem.id)}</span>
                        <div>
                            ${(mem.tags || []).map(t => `<span class="badge badge-blue">${escapeHtml(t)}</span>`).join(' ')}
                            <button class="btn btn-sm btn-danger" onclick="window._deleteMemory('${escapeHtml(mem.id)}')">Delete</button>
                        </div>
                    </div>
                    <div class="card-body">${escapeHtml(mem.content)}</div>
                    <div class="card-meta">${mem.created_at || ''} ${mem.score != null ? `— relevance: ${(mem.score * 100).toFixed(0)}%` : ''}</div>
                </div>
            `).join('');
        } catch (e) {
            toast(`Failed to load memory: ${e.message}`, 'error');
        }
    }

    window._deleteMemory = async function (id) {
        try {
            await api.del(`/memory/${id}`);
            toast('Memory deleted', 'success');
            loadMemory();
        } catch (e) {
            toast(`Delete failed: ${e.message}`, 'error');
        }
    };

    // ── Contracts ─────────────────────────────────────────────────────────

    async function loadContracts() {
        try {
            const data = await api.get('/contracts');
            const contracts = Array.isArray(data) ? data : (data.contracts || []);
            const container = document.getElementById('contracts-list');

            document.getElementById('contracts-count-badge').textContent = `${contracts.length} contracts`;

            if (contracts.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No contracts defined. Contracts enforce rules like token limits, cost caps, and tool restrictions on agent behavior.</p></div>';
                return;
            }

            container.innerHTML = contracts.map(c => {
                const enfBadge = c.enforcement === 'block' ? 'badge-red'
                    : c.enforcement === 'warn' ? 'badge-orange' : 'badge-blue';
                const ruleDisplay = typeof c.rule === 'object' ? JSON.stringify(c.rule) : (c.rule || '—');

                return `
                    <div class="card">
                        <div class="card-header">
                            <span class="card-title">🛡️ ${escapeHtml(c.name)}</span>
                            <div>
                                <span class="badge badge-purple">${escapeHtml(c.scope || 'global')}</span>
                                <span class="badge ${enfBadge}">${escapeHtml(c.enforcement || 'log')}</span>
                            </div>
                        </div>
                        <div class="card-body">
                            <div><strong>Rule:</strong> <code style="color:var(--accent)">${escapeHtml(c.rule_type || '')}</code></div>
                            <div style="font-family:var(--font-mono);font-size:12px;margin-top:4px;color:var(--text-dim)">${escapeHtml(ruleDisplay)}</div>
                        </div>
                        <div class="card-actions">
                            <button class="btn btn-sm btn-danger" onclick="window._deleteContract('${escapeHtml(c.name)}')">Delete</button>
                        </div>
                    </div>
                `;
            }).join('');
        } catch (e) {
            toast(`Failed to load contracts: ${e.message}`, 'error');
        }
    }

    window._deleteContract = async function (name) {
        try {
            await api.del(`/contracts/${encodeURIComponent(name)}`);
            toast(`Contract "${name}" deleted`, 'success');
            loadContracts();
        } catch (e) {
            toast(`Delete failed: ${e.message}`, 'error');
        }
    };

    async function createContract() {
        const name = document.getElementById('contract-name').value.trim();
        const scope = document.getElementById('contract-scope').value;
        const enforcement = document.getElementById('contract-enforcement').value;
        const ruleType = document.getElementById('contract-rule-type').value;
        const ruleValueRaw = document.getElementById('contract-rule-value').value.trim();

        if (!name) { toast('Contract name required', 'error'); return; }

        let ruleValue;
        try {
            ruleValue = ruleValueRaw ? JSON.parse(ruleValueRaw) : {};
        } catch {
            toast('Invalid JSON in rule value', 'error');
            return;
        }

        try {
            await api.post('/contracts', {
                name,
                scope,
                enforcement,
                rule_type: ruleType,
                rule: ruleValue,
            });
            toast(`Contract "${name}" created!`, 'success');
            document.getElementById('contract-form').style.display = 'none';
            ['contract-name', 'contract-rule-value'].forEach(id => document.getElementById(id).value = '');
            loadContracts();
        } catch (e) {
            toast(`Failed: ${e.message}`, 'error');
        }
    }

    // ── Usage & Cost Tracking ─────────────────────────────────────────────

    async function loadUsage() {
        try {
            const [usage, budgets, traces] = await Promise.all([
                api.get('/usage'),
                api.get('/budgets').catch(() => []),
                api.get('/traces').catch(() => []),
            ]);

            // Stats
            const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
            set('usage-total-cost', formatCost(usage.total_cost || usage.session_cost_usd || 0));
            set('usage-input-tokens', formatNumber(usage.total_input_tokens || usage.input_tokens || 0));
            set('usage-output-tokens', formatNumber(usage.total_output_tokens || usage.output_tokens || 0));
            set('usage-requests', formatNumber(usage.total_requests || usage.requests || 0));

            const budgetArr = Array.isArray(budgets) ? budgets : (budgets.budgets || []);
            set('usage-budgets', budgetArr.length);

            // Traces
            const traceArr = Array.isArray(traces) ? traces : (traces.traces || []);
            const tracesContainer = document.getElementById('traces-list');

            if (traceArr.length === 0) {
                tracesContainer.innerHTML = '<div class="empty-state"><p>No execution traces yet. Traces appear when the agent processes requests.</p></div>';
            } else {
                tracesContainer.innerHTML = `
                    <table class="data-table">
                        <tr><th>ID</th><th>Model</th><th>Tokens</th><th>Cost</th><th>Duration</th><th>Status</th></tr>
                        ${traceArr.slice(0, 50).map(t => {
                            const totalTokens = (t.input_tokens || 0) + (t.output_tokens || 0);
                            return `
                                <tr>
                                    <td class="clickable" onclick="window._showTrace('${escapeHtml(t.id || t.trace_id || '')}')">${escapeHtml((t.id || t.trace_id || '').slice(0, 8))}…</td>
                                    <td>${escapeHtml(t.model || '—')}</td>
                                    <td>${formatNumber(totalTokens)}</td>
                                    <td style="color:var(--accent)">${formatCost(t.cost || 0)}</td>
                                    <td>${t.duration_ms != null ? t.duration_ms + 'ms' : '—'}</td>
                                    <td><span class="badge ${t.status === 'ok' || t.success ? 'badge-green' : 'badge-red'}">${escapeHtml(t.status || (t.success ? 'ok' : 'error'))}</span></td>
                                </tr>
                            `;
                        }).join('')}
                    </table>
                `;
            }

            // Budgets
            const budgetsContainer = document.getElementById('budgets-list');

            if (budgetArr.length === 0) {
                budgetsContainer.innerHTML = '<div class="empty-state" style="padding:16px"><p>No budgets set</p></div>';
            } else {
                budgetsContainer.innerHTML = budgetArr.map(b => {
                    const spent = b.spent || b.current_cost || 0;
                    const limit = b.limit || b.max_cost || 1;
                    const pct = Math.min(100, (spent / limit) * 100);
                    const barClass = pct > 90 ? 'danger' : pct > 70 ? 'warn' : '';

                    return `
                        <div class="budget-card">
                            <div class="budget-header">
                                <span class="budget-scope">${escapeHtml(b.scope || 'default')}</span>
                                <button class="btn btn-sm btn-danger" onclick="window._deleteBudget('${escapeHtml(b.scope || '')}')">✕</button>
                            </div>
                            <div class="budget-bar"><div class="budget-bar-fill ${barClass}" style="width:${pct}%"></div></div>
                            <div class="budget-label">
                                <span>${formatCost(spent)} spent</span>
                                <span>${formatCost(limit)} limit</span>
                            </div>
                        </div>
                    `;
                }).join('');
            }
        } catch (e) {
            toast(`Failed to load usage: ${e.message}`, 'error');
        }
    }

    window._showTrace = async function (id) {
        if (!id) return;
        const modal = document.getElementById('trace-modal');
        const body = document.getElementById('trace-modal-body');
        const title = document.getElementById('trace-modal-title');
        title.textContent = `Trace: ${id.slice(0, 16)}…`;
        body.textContent = 'Loading...';
        modal.style.display = 'flex';

        try {
            const data = await api.get(`/traces/${id}`);
            body.textContent = JSON.stringify(data, null, 2);
        } catch (e) {
            body.textContent = `Error loading trace: ${e.message}`;
        }
    };

    window._deleteBudget = async function (scope) {
        try {
            await api.del(`/budgets/${encodeURIComponent(scope)}`);
            toast(`Budget "${scope}" deleted`, 'success');
            loadUsage();
        } catch (e) {
            toast(`Delete failed: ${e.message}`, 'error');
        }
    };

    async function createBudget() {
        const scope = document.getElementById('budget-scope').value.trim();
        const maxCost = parseFloat(document.getElementById('budget-max-cost').value);
        if (!scope) { toast('Budget scope required', 'error'); return; }
        if (isNaN(maxCost) || maxCost <= 0) { toast('Valid max cost required', 'error'); return; }

        try {
            await api.post('/budgets', { scope, max_cost: maxCost });
            toast(`Budget "${scope}" created!`, 'success');
            document.getElementById('budget-form').style.display = 'none';
            document.getElementById('budget-scope').value = '';
            document.getElementById('budget-max-cost').value = '';
            loadUsage();
        } catch (e) {
            toast(`Failed: ${e.message}`, 'error');
        }
    }

    // ── Channels ──────────────────────────────────────────────────────────

    async function loadChannels() {
        try {
            const data = await api.get('/channels');
            const container = document.getElementById('channels-list');
            const channels = data.channels || [];

            if (channels.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No channels configured. Add channels in your config to enable Telegram, Discord, Slack, and more.</p></div>';
                return;
            }

            container.innerHTML = channels.map(ch => {
                const name = ch.name || ch;
                const enabled = ch.enabled !== false;
                const connected = ch.connected === true;
                const health = ch.health || 'unknown';
                const healthBadge = health === 'healthy' ? 'badge-green'
                    : health === 'degraded' ? 'badge-orange' : 'badge-red';

                return `
                    <div class="channel-card">
                        <div class="channel-info">
                            <div class="channel-icon">${channelIcon(name)}</div>
                            <div>
                                <div class="channel-name">${escapeHtml(name)}</div>
                                <div class="channel-meta">
                                    <span class="badge ${enabled ? 'badge-green' : 'badge-red'}">${enabled ? 'Enabled' : 'Disabled'}</span>
                                    <span class="badge ${connected ? 'badge-green' : 'badge-orange'}">${connected ? 'Connected' : 'Disconnected'}</span>
                                    <span class="badge ${healthBadge}">${escapeHtml(health)}</span>
                                </div>
                            </div>
                        </div>
                        <div class="channel-actions">
                            <button class="btn btn-sm btn-success" onclick="window._testChannel('${escapeHtml(name)}')">Test</button>
                        </div>
                    </div>
                `;
            }).join('');
        } catch (e) {
            toast(`Failed to load channels: ${e.message}`, 'error');
        }
    }

    window._testChannel = async function (name) {
        try {
            const result = await api.post(`/channels/${encodeURIComponent(name)}/test`, {});
            const success = result.success !== false;
            toast(`Channel "${name}": ${success ? 'Test passed ✓' : 'Test failed ✗'}`, success ? 'success' : 'error');
        } catch (e) {
            toast(`Channel test failed: ${e.message}`, 'error');
        }
    };

    // ── Routines ──────────────────────────────────────────────────────────

    async function loadRoutines() {
        try {
            const data = await api.get('/routines');
            const container = document.getElementById('routines-list');
            const routines = data.routines || [];
            if (routines.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No routines configured. Create routines to schedule periodic agent tasks.</p></div>';
                return;
            }
            container.innerHTML = routines.map(r => `
                <div class="card">
                    <div class="card-header">
                        <span class="card-title">⏰ ${escapeHtml(r.name)}</span>
                        <span class="badge ${r.enabled ? 'badge-green' : 'badge-red'}">
                            ${r.enabled ? 'Enabled' : 'Disabled'}
                        </span>
                    </div>
                    <div class="card-body">
                        <div><strong>Schedule:</strong> <code style="color:var(--accent)">${escapeHtml(r.schedule || 'N/A')}</code></div>
                        <div><strong>Action:</strong> ${escapeHtml(r.action || r.instruction || '')}</div>
                        ${r.last_run ? `<div class="card-meta">Last run: ${escapeHtml(r.last_run)}</div>` : ''}
                    </div>
                    <div class="card-actions">
                        <button class="btn btn-sm btn-danger" onclick="window._deleteRoutine('${escapeHtml(r.id || r.name)}')">Delete</button>
                    </div>
                </div>
            `).join('');
        } catch (e) {
            toast(`Failed to load routines: ${e.message}`, 'error');
        }
    }

    window._deleteRoutine = async function (id) {
        try {
            await api.del(`/routines/${encodeURIComponent(id)}`);
            toast('Routine deleted', 'success');
            loadRoutines();
        } catch (e) {
            toast(`Delete failed: ${e.message}`, 'error');
        }
    };

    // ── Jobs ──────────────────────────────────────────────────────────────

    async function loadJobs() {
        try {
            const data = await api.get('/jobs');
            const container = document.getElementById('jobs-list');
            const jobs = data.jobs || [];

            document.getElementById('jobs-count-badge').textContent = `${jobs.length} jobs`;

            if (jobs.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No jobs. Jobs are created when routines execute.</p></div>';
                return;
            }

            container.innerHTML = jobs.map(j => {
                const badgeClass = j.status === 'Completed' || j.status === 'completed' ? 'badge-green'
                    : j.status === 'Failed' || j.status === 'failed' ? 'badge-red'
                    : j.status === 'Running' || j.status === 'running' ? 'badge-orange' : 'badge-blue';
                return `
                    <div class="card">
                        <div class="card-header">
                            <span class="card-title">📋 ${escapeHtml(j.id || j.job_id || '—')}</span>
                            <span class="badge ${badgeClass}">${escapeHtml(j.status)}</span>
                        </div>
                        <div class="card-body">
                            <div><strong>Routine:</strong> ${escapeHtml(j.routine_id || j.routine || '—')}</div>
                            ${j.started_at ? `<div><strong>Started:</strong> ${escapeHtml(j.started_at)}</div>` : ''}
                            ${j.completed_at ? `<div><strong>Completed:</strong> ${escapeHtml(j.completed_at)}</div>` : ''}
                            ${j.duration_ms != null ? `<div><strong>Duration:</strong> ${j.duration_ms}ms</div>` : ''}
                            ${j.error ? `<div style="color:var(--red);margin-top:4px"><strong>Error:</strong> ${escapeHtml(j.error)}</div>` : ''}
                        </div>
                    </div>
                `;
            }).join('');
        } catch (e) {
            toast(`Failed to load jobs: ${e.message}`, 'error');
        }
    }

    // ── Logs ──────────────────────────────────────────────────────────────

    function connectLogStream() {
        if (state.logSource) state.logSource.close();

        const container = document.getElementById('log-container');
        container.innerHTML = '';

        state.logSource = new EventSource('/v1/logs');

        const eventTypes = [
            'tool_executed', 'response_generated', 'error_occurred',
            'memory_accessed', 'message_received', 'agent_state_changed',
            'contract_violation', 'budget_exceeded',
        ];

        eventTypes.forEach(type => {
            state.logSource.addEventListener(type, (e) => appendLog(type, e.data));
        });

        state.logSource.onerror = () => { /* SSE auto-reconnect */ };
    }

    function appendLog(type, data) {
        const container = document.getElementById('log-container');
        const empty = container.querySelector('.empty-state');
        if (empty) empty.remove();

        const now = new Date().toLocaleTimeString();
        const el = document.createElement('div');
        el.className = 'log-entry';
        el.setAttribute('data-type', type);

        // Apply filter
        if (state.logFilter && type !== state.logFilter) {
            el.classList.add('hidden');
        }

        let detail = '';
        try {
            const parsed = JSON.parse(data);
            detail = Object.entries(parsed)
                .filter(([k]) => k !== 'timestamp')
                .map(([k, v]) => `${k}=${typeof v === 'object' ? JSON.stringify(v) : v}`)
                .join(' ');
        } catch {
            detail = data;
        }

        el.innerHTML = `
            <span class="log-time">${now}</span>
            <span class="log-type ${type}">${type}</span>
            <span>${escapeHtml(detail)}</span>
        `;

        container.appendChild(el);

        if (document.getElementById('log-autoscroll').checked) {
            container.scrollTop = container.scrollHeight;
        }
    }

    function applyLogFilter(filter) {
        state.logFilter = filter;
        document.querySelectorAll('.log-entry').forEach(el => {
            if (!filter) {
                el.classList.remove('hidden');
            } else {
                el.classList.toggle('hidden', el.getAttribute('data-type') !== filter);
            }
        });
    }

    // ── Settings ──────────────────────────────────────────────────────────

    async function loadSettings() {
        try {
            const [configData, statusData] = await Promise.all([
                api.get('/config'),
                api.get('/status'),
            ]);

            // Config editor
            const config = configData.config || configData;
            document.getElementById('config-editor').value = JSON.stringify(config, null, 2);

            // Status
            const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
            set('status-status', statusData.status || 'unknown');
            set('status-uptime', formatUptime(statusData.uptime_secs || 0));
            set('status-conversations', statusData.active_conversations || 0);
            set('status-memory', statusData.memory_entries || 0);
            set('status-documents', statusData.document_entries || 0);
            set('status-tools', statusData.tools_count || 0);
            set('status-contracts', statusData.contracts_count || 0);
            set('status-cost', formatCost(statusData.session_cost_usd));
            set('status-traces', statusData.trace_count || 0);
            set('status-provider', statusData.provider || '—');
            set('status-workflow', statusData.workflow_engine ? 'Enabled' : 'Disabled');

            updateStatus(true);
        } catch (e) {
            toast(`Failed to load settings: ${e.message}`, 'error');
        }
    }

    async function saveSettings() {
        const raw = document.getElementById('config-editor').value.trim();
        let config;
        try {
            config = JSON.parse(raw);
        } catch {
            toast('Invalid JSON in configuration', 'error');
            return;
        }

        try {
            await api.patch('/config', config);
            toast('Configuration saved!', 'success');
            loadSettings();
        } catch (e) {
            toast(`Save failed: ${e.message}`, 'error');
        }
    }

    // ── Status ────────────────────────────────────────────────────────────

    function updateStatus(online) {
        const indicator = document.getElementById('status-indicator');
        const text = indicator.querySelector('.status-text');
        indicator.className = `status ${online ? 'online' : 'offline'}`;
        text.textContent = online ? 'Connected' : 'Disconnected';
    }

    async function checkHealth() {
        try {
            const res = await fetch('/health');
            updateStatus(res.ok);
        } catch {
            updateStatus(false);
        }
    }

    // ── Event Bindings ────────────────────────────────────────────────────

    function init() {
        // Navigation
        document.querySelectorAll('.nav-link').forEach(el => {
            el.addEventListener('click', (e) => {
                e.preventDefault();
                navigateTo(el.dataset.page);
            });
        });

        // Chat
        document.getElementById('send-btn').addEventListener('click', sendMessage);
        document.getElementById('chat-input').addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                sendMessage();
            }
        });

        document.getElementById('new-chat-btn').addEventListener('click', () => {
            state.conversationId = null;
            document.getElementById('chat-messages').innerHTML =
                '<div class="empty-state"><p>Start a conversation with the agent</p></div>';
        });

        // Memory
        document.getElementById('memory-search-btn').addEventListener('click', () => {
            loadMemory(document.getElementById('memory-search').value.trim() || undefined);
        });
        document.getElementById('memory-search').addEventListener('keydown', (e) => {
            if (e.key === 'Enter') loadMemory(e.target.value.trim() || undefined);
        });
        document.getElementById('new-memory-btn').addEventListener('click', () => {
            document.getElementById('memory-save-form').style.display = 'block';
        });
        document.getElementById('memory-cancel-btn').addEventListener('click', () => {
            document.getElementById('memory-save-form').style.display = 'none';
        });
        document.getElementById('memory-save-btn').addEventListener('click', async () => {
            const content = document.getElementById('memory-content').value.trim();
            if (!content) { toast('Enter memory content', 'error'); return; }
            const tags = document.getElementById('memory-tags').value.trim()
                .split(',').map(t => t.trim()).filter(Boolean);
            try {
                await api.post('/memory', { content, tags, agent_id: 'default' });
                toast('Memory saved!', 'success');
                document.getElementById('memory-save-form').style.display = 'none';
                document.getElementById('memory-content').value = '';
                document.getElementById('memory-tags').value = '';
                loadMemory();
            } catch (e) { toast('Failed: ' + e.message, 'error'); }
        });

        // Tools
        document.getElementById('refresh-tools-btn').addEventListener('click', loadTools);

        // Contracts
        document.getElementById('new-contract-btn').addEventListener('click', () => {
            document.getElementById('contract-form').style.display = 'block';
        });
        document.getElementById('contract-cancel-btn').addEventListener('click', () => {
            document.getElementById('contract-form').style.display = 'none';
        });
        document.getElementById('contract-save-btn').addEventListener('click', createContract);

        // Usage
        document.getElementById('refresh-usage-btn').addEventListener('click', loadUsage);
        document.getElementById('refresh-traces-btn').addEventListener('click', loadUsage);
        document.getElementById('new-budget-btn').addEventListener('click', () => {
            document.getElementById('budget-form').style.display = 'block';
        });
        document.getElementById('budget-cancel-btn').addEventListener('click', () => {
            document.getElementById('budget-form').style.display = 'none';
        });
        document.getElementById('budget-save-btn').addEventListener('click', createBudget);

        // Channels
        document.getElementById('refresh-channels-btn').addEventListener('click', loadChannels);

        // Routines
        document.getElementById('new-routine-btn').addEventListener('click', () => {
            document.getElementById('routine-form').style.display = 'block';
        });
        document.getElementById('routine-cancel-btn').addEventListener('click', () => {
            document.getElementById('routine-form').style.display = 'none';
        });
        document.getElementById('routine-save-btn').addEventListener('click', async () => {
            const name = document.getElementById('routine-name').value.trim();
            const schedule = document.getElementById('routine-schedule').value.trim();
            const instruction = document.getElementById('routine-instruction').value.trim();
            if (!name || !schedule || !instruction) { toast('Fill in all fields', 'error'); return; }
            try {
                await api.post('/routines', { name, schedule, instruction, enabled: true });
                toast('Routine created!', 'success');
                document.getElementById('routine-form').style.display = 'none';
                ['routine-name', 'routine-schedule', 'routine-instruction'].forEach(id =>
                    document.getElementById(id).value = '');
                loadRoutines();
            } catch (e) { toast('Failed: ' + e.message, 'error'); }
        });

        // Jobs
        document.getElementById('refresh-jobs-btn').addEventListener('click', loadJobs);

        // Logs
        document.getElementById('clear-logs-btn').addEventListener('click', () => {
            document.getElementById('log-container').innerHTML = '';
        });
        document.getElementById('log-filter').addEventListener('change', (e) => {
            applyLogFilter(e.target.value);
        });

        // Settings
        document.getElementById('save-settings-btn').addEventListener('click', saveSettings);

        // Dashboard
        document.getElementById('refresh-dashboard-btn').addEventListener('click', loadDashboard);

        // Trace modal close
        document.getElementById('trace-modal-close').addEventListener('click', () => {
            document.getElementById('trace-modal').style.display = 'none';
        });
        document.getElementById('trace-modal').addEventListener('click', (e) => {
            if (e.target === e.currentTarget) {
                document.getElementById('trace-modal').style.display = 'none';
            }
        });

        // Health check every 30s
        checkHealth();
        setInterval(checkHealth, 30000);

        // Initial page
        navigateTo('dashboard');
    }

    // ── Boot ──────────────────────────────────────────────────────────────

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();

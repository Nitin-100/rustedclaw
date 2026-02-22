// ── RustedClaw Frontend Application ─────────────────────────────────────────
// Vanilla JS — no build step, connects to /v1/* API endpoints

(function () {
    'use strict';

    // ── State ─────────────────────────────────────────────────────────────

    const state = {
        currentPage: 'chat',
        conversationId: null,
        ws: null,
        logSource: null,
        isStreaming: false,
    };

    // ── API Helpers ───────────────────────────────────────────────────────

    const api = {
        async get(path) {
            const res = await fetch(`/v1${path}`);
            if (!res.ok) throw new Error(`API error: ${res.status}`);
            return res.json();
        },

        async post(path, body) {
            const res = await fetch(`/v1${path}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            if (!res.ok) throw new Error(`API error: ${res.status}`);
            return res.json();
        },

        async patch(path, body) {
            const res = await fetch(`/v1${path}`, {
                method: 'PATCH',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            if (!res.ok) throw new Error(`API error: ${res.status}`);
            return res.json();
        },

        async del(path) {
            const res = await fetch(`/v1${path}`, { method: 'DELETE' });
            if (!res.ok) throw new Error(`API error: ${res.status}`);
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

        // Load data for the page
        switch (page) {
            case 'tools':    loadTools(); break;
            case 'memory':   loadMemory(); break;
            case 'routines': loadRoutines(); break;
            case 'jobs':     loadJobs(); break;
            case 'logs':     connectLogStream(); break;
            case 'settings': loadSettings(); break;
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

    // ── Chat ──────────────────────────────────────────────────────────────

    function addChatMessage(role, content) {
        const container = document.getElementById('chat-messages');

        // Remove empty state
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
            <span class="badge ${success ? 'badge-green' : 'badge-red'}">${success ? 'Success' : 'Failed'}</span>
            <div class="tool-card">
                <div><strong>Input:</strong> ${escapeHtml(typeof input === 'string' ? input : JSON.stringify(input))}</div>
                <div><strong>Output:</strong> ${escapeHtml(output)}</div>
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

            if (!response.ok) {
                throw new Error(`Stream error: ${response.status}`);
            }

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
                            const container = document.getElementById('chat-messages');
                            container.scrollTop = container.scrollHeight;
                            break;
                        }
                        case 'tool_call': {
                            const data = JSON.parse(event.data);
                            addChatMessage('system', `Calling tool: ${data.name}`);
                            break;
                        }
                        case 'tool_result': {
                            const data = JSON.parse(event.data);
                            addToolCard(data.name, data.input || '', data.output, data.success);
                            break;
                        }
                        case 'done': {
                            const data = JSON.parse(event.data);
                            if (data.conversation_id) {
                                state.conversationId = data.conversation_id;
                            }
                            if (assistantEl) {
                                assistantEl.classList.remove('streaming-cursor');
                            }
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

            // If no chunks came through (fallback), ensure cursor is removed
            if (assistantEl) {
                assistantEl.classList.remove('streaming-cursor');
            }
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

        // Keep unparsed remainder for next chunk
        if (currentEvent) {
            const lastNewline = text.lastIndexOf('\n\n');
            if (lastNewline >= 0) {
                remainder = text.slice(lastNewline + 2);
            } else {
                remainder = text;
            }
        }

        return { parsed, remainder };
    }

    // ── Tools ─────────────────────────────────────────────────────────────

    async function loadTools() {
        try {
            const data = await api.get('/tools');
            const container = document.getElementById('tools-list');
            if (!data.tools || data.tools.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No tools registered</p></div>';
                return;
            }
            container.innerHTML = data.tools.map(tool => `
                <div class="card">
                    <div class="card-header">
                        <span class="card-title">🔧 ${escapeHtml(tool.name)}</span>
                    </div>
                    <div class="card-body">${escapeHtml(tool.description)}</div>
                </div>
            `).join('');
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
                            <button class="btn btn-sm btn-danger" onclick="deleteMemory('${escapeHtml(mem.id)}')">Delete</button>
                        </div>
                    </div>
                    <div class="card-body">${escapeHtml(mem.content)}</div>
                    <div class="card-meta">${mem.created_at || ''} — score: ${mem.score || 0}</div>
                </div>
            `).join('');
        } catch (e) {
            toast(`Failed to load memory: ${e.message}`, 'error');
        }
    }

    window.deleteMemory = async function (id) {
        try {
            await api.del(`/memory/${id}`);
            toast('Memory deleted', 'success');
            loadMemory();
        } catch (e) {
            toast(`Delete failed: ${e.message}`, 'error');
        }
    };

    // ── Routines ──────────────────────────────────────────────────────────

    async function loadRoutines() {
        try {
            const data = await api.get('/routines');
            const container = document.getElementById('routines-list');
            const routines = data.routines || [];
            if (routines.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No routines configured</p></div>';
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
                        <div>Schedule: <code>${escapeHtml(r.schedule || 'N/A')}</code></div>
                        <div>Action: ${escapeHtml(r.action || '')}</div>
                    </div>
                </div>
            `).join('');
        } catch (e) {
            toast(`Failed to load routines: ${e.message}`, 'error');
        }
    }

    // ── Jobs ──────────────────────────────────────────────────────────────

    async function loadJobs() {
        try {
            const data = await api.get('/jobs');
            const container = document.getElementById('jobs-list');
            const jobs = data.jobs || [];
            if (jobs.length === 0) {
                container.innerHTML = '<div class="empty-state"><p>No jobs</p></div>';
                return;
            }
            container.innerHTML = jobs.map(j => {
                const badgeClass = j.status === 'Completed' ? 'badge-green'
                    : j.status === 'Failed' ? 'badge-red' : 'badge-orange';
                return `
                    <div class="card">
                        <div class="card-header">
                            <span class="card-title">📋 ${escapeHtml(j.id)}</span>
                            <span class="badge ${badgeClass}">${escapeHtml(j.status)}</span>
                        </div>
                        <div class="card-body">
                            <div>Routine: ${escapeHtml(j.routine_id)}</div>
                            ${j.error ? `<div style="color:var(--red)">Error: ${escapeHtml(j.error)}</div>` : ''}
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
        if (state.logSource) {
            state.logSource.close();
        }

        const container = document.getElementById('log-container');
        container.innerHTML = '';

        state.logSource = new EventSource('/v1/logs');

        state.logSource.addEventListener('tool_executed', (e) => appendLog('tool_executed', e.data));
        state.logSource.addEventListener('response_generated', (e) => appendLog('response_generated', e.data));
        state.logSource.addEventListener('error_occurred', (e) => appendLog('error_occurred', e.data));
        state.logSource.addEventListener('memory_accessed', (e) => appendLog('memory_accessed', e.data));
        state.logSource.addEventListener('message_received', (e) => appendLog('message_received', e.data));
        state.logSource.addEventListener('agent_state_changed', (e) => appendLog('agent_state_changed', e.data));

        state.logSource.onerror = () => {
            // SSE will auto-reconnect
        };
    }

    function appendLog(type, data) {
        const container = document.getElementById('log-container');

        // Remove empty state
        const empty = container.querySelector('.empty-state');
        if (empty) empty.remove();

        const now = new Date().toLocaleTimeString();
        const el = document.createElement('div');
        el.className = 'log-entry';

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

    // ── Settings ──────────────────────────────────────────────────────────

    async function loadSettings() {
        try {
            const [configData, statusData, channelsData] = await Promise.all([
                api.get('/config'),
                api.get('/status'),
                api.get('/channels'),
            ]);

            // Config
            document.getElementById('config-display').textContent =
                JSON.stringify(configData.config || configData, null, 2);

            // Status
            document.getElementById('status-status').textContent = statusData.status || 'unknown';
            document.getElementById('status-uptime').textContent = formatUptime(statusData.uptime_secs || 0);
            document.getElementById('status-conversations').textContent = statusData.active_conversations || 0;
            document.getElementById('status-memory').textContent = statusData.memory_entries || 0;
            document.getElementById('status-documents').textContent = statusData.document_entries || 0;
            document.getElementById('status-tools').textContent = statusData.tools_count || 0;
            document.getElementById('status-provider').textContent = statusData.provider || '—';

            // Channels
            const channelsList = document.getElementById('channels-list');
            const channels = channelsData.channels || [];
            if (channels.length === 0) {
                channelsList.innerHTML = '<div class="empty-state"><p>No channels configured</p></div>';
            } else {
                channelsList.innerHTML = channels.map(ch => `
                    <div class="card">
                        <div class="card-header">
                            <span class="card-title">${escapeHtml(ch.name || ch)}</span>
                        </div>
                    </div>
                `).join('');
            }

            // Update global status
            updateStatus(true);
        } catch (e) {
            toast(`Failed to load settings: ${e.message}`, 'error');
        }
    }

    function formatUptime(secs) {
        const h = Math.floor(secs / 3600);
        const m = Math.floor((secs % 3600) / 60);
        const s = Math.floor(secs % 60);
        if (h > 0) return `${h}h ${m}m ${s}s`;
        if (m > 0) return `${m}m ${s}s`;
        return `${s}s`;
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

    // ── Utilities ─────────────────────────────────────────────────────────

    function escapeHtml(str) {
        if (typeof str !== 'string') return '';
        const div = document.createElement('div');
        div.textContent = str;
        return div.innerHTML;
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

        // Memory search
        document.getElementById('memory-search-btn').addEventListener('click', () => {
            const query = document.getElementById('memory-search').value.trim();
            loadMemory(query || undefined);
        });

        document.getElementById('memory-search').addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
                const query = e.target.value.trim();
                loadMemory(query || undefined);
            }
        });

        // Tools refresh
        document.getElementById('refresh-tools-btn').addEventListener('click', loadTools);

        // New Routine form
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
                document.getElementById('routine-name').value = '';
                document.getElementById('routine-schedule').value = '';
                document.getElementById('routine-instruction').value = '';
                loadRoutines();
            } catch (e) { toast('Failed: ' + e.message, 'error'); }
        });

        // Save Memory form
        document.getElementById('new-memory-btn').addEventListener('click', () => {
            document.getElementById('memory-save-form').style.display = 'block';
        });
        document.getElementById('memory-cancel-btn').addEventListener('click', () => {
            document.getElementById('memory-save-form').style.display = 'none';
        });
        document.getElementById('memory-save-btn').addEventListener('click', async () => {
            const content = document.getElementById('memory-content').value.trim();
            const tagsStr = document.getElementById('memory-tags').value.trim();
            if (!content) { toast('Enter memory content', 'error'); return; }
            const tags = tagsStr ? tagsStr.split(',').map(t => t.trim()).filter(Boolean) : [];
            try {
                await api.post('/memory', { content, tags, agent_id: 'default' });
                toast('Memory saved!', 'success');
                document.getElementById('memory-save-form').style.display = 'none';
                document.getElementById('memory-content').value = '';
                document.getElementById('memory-tags').value = '';
                loadMemory();
            } catch (e) { toast('Failed: ' + e.message, 'error'); }
        });
        document.getElementById('memory-search-btn').addEventListener('click', () => {
            const query = document.getElementById('memory-search').value.trim();
            loadMemory(query || undefined);
        });

        // Jobs refresh
        document.getElementById('refresh-jobs-btn').addEventListener('click', loadJobs);

        // Clear logs
        document.getElementById('clear-logs-btn').addEventListener('click', () => {
            document.getElementById('log-container').innerHTML = '';
        });

        // Health check every 30s
        checkHealth();
        setInterval(checkHealth, 30000);

        // Initial page
        navigateTo('chat');
    }

    // ── Boot ──────────────────────────────────────────────────────────────

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();

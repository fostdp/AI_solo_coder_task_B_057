let GLOBAL_STATE = {
    relics: [],
    sensors: [],
    latestValues: [],
    charts: {},
    cachedContour: null,
    currentRelicId: null,
    alertSet: new Set(),
    refreshTimer: null,
    clockTimer: null,
};

const API = {
    async get(path, params = {}) {
        const url = new URL(API_CONFIG.BASE_URL + path);
        Object.entries(params).forEach(([k, v]) => v !== undefined && v !== null && url.searchParams.set(k, v));
        try {
            const resp = await fetch(url.toString());
            return await resp.json();
        } catch (e) {
            console.warn('API请求失败:', path, e);
            return { success: false, message: e.message };
        }
    },
    async post(path, body) {
        try {
            const resp = await fetch(API_CONFIG.BASE_URL + path, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(body),
            });
            return await resp.json();
        } catch (e) {
            console.warn('API POST失败:', path, e);
            return { success: false, message: e.message };
        }
    }
};

function switchTab(tabName) {
    document.querySelectorAll('.tab').forEach(t => t.classList.toggle('active', t.dataset.tab === tabName));
    document.querySelectorAll('.tab-panel').forEach(p => p.classList.toggle('active', p.id === `panel-${tabName}`));

    if (tabName === 'relic' && !GLOBAL_STATE.charts.pointcloudInited) {
        onTabRelic();
        GLOBAL_STATE.charts.pointcloudInited = true;
    }
    if (tabName === 'contour') renderContourView();
    if (tabName === 'grid') renderGridView();
    if (tabName === 'analysis') renderAnalysisView();
    if (tabName === 'alerts') loadAlertsTable();
    if (tabName === 'sensors') loadSensorsTable();
}

document.addEventListener('DOMContentLoaded', function () {
    initClock();
    document.querySelectorAll('.tab').forEach(tab => {
        tab.addEventListener('click', () => switchTab(tab.dataset.tab));
    });
    document.getElementById('refresh-btn').addEventListener('click', refreshDashboard);

    bindDashboardInputs();
    bindPointCloudControls();
    bindContourControls();
    bindAlertActions();

    loadAllBaseData();
    refreshDashboard();

    if (API_CONFIG.AUTO_REFRESH) {
        GLOBAL_STATE.refreshTimer = setInterval(refreshDashboard, API_CONFIG.REFRESH_INTERVAL_MS);
    }
});

function initClock() {
    const el = document.getElementById('clock');
    function update() {
        const d = new Date();
        const pad = n => String(n).padStart(2, '0');
        el.textContent = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
    }
    update();
    GLOBAL_STATE.clockTimer = setInterval(update, 1000);
}

async function loadAllBaseData() {
    try {
        const health = await API.get('/api/health');
        if (health?.success) {
            const ds = health.data?.database || 'unknown';
            document.getElementById('db-status').textContent = ds === 'connected' ? '已连接' : '未连接';
            document.getElementById('db-status').className = ds === 'connected' ? '' : 'dot err';
        }
    } catch (e) {}

    try {
        const relicsResp = await API.get('/api/relics');
        if (relicsResp?.success && relicsResp.data) {
            GLOBAL_STATE.relics = relicsResp.data;
            populateRelicSelect();
        }
    } catch (e) {}

    try {
        const sensorsResp = await API.get('/api/sensors');
        if (sensorsResp?.success && sensorsResp.data) {
            GLOBAL_STATE.sensors = sensorsResp.data;
            renderSensorMiniMap(sensorsResp.data);
        }
    } catch (e) {}
}

function populateRelicSelect() {
    const sel = document.getElementById('relic-select');
    if (!sel) return;
    sel.innerHTML = '';
    GLOBAL_STATE.relics.slice(0, 200).forEach(r => {
        const opt = document.createElement('option');
        opt.value = r.id;
        opt.textContent = `${r.id} - ${r.name} (${r.category})`;
        sel.appendChild(opt);
    });
    if (GLOBAL_STATE.relics.length > 0) {
        GLOBAL_STATE.currentRelicId = GLOBAL_STATE.relics[0].id;
        sel.value = GLOBAL_STATE.currentRelicId;
    }
}

async function refreshDashboard() {
    try {
        const stats = await API.get('/api/stats/summary');
        if (stats?.success && stats.data) {
            updateDashboardStats(stats.data);
        }
    } catch (e) {}

    try {
        const latestResp = await API.get('/api/sensors/latest');
        if (latestResp?.success && latestResp.data) {
            GLOBAL_STATE.latestValues = latestResp.data;
            updateEnvironmentCards(latestResp.data);
        }
    } catch (e) {}

    try {
        const alertsResp = await API.get('/api/alerts', { limit: 8 });
        if (alertsResp?.success && alertsResp.data) {
            renderRecentAlerts(alertsResp.data.alerts || []);
        }
    } catch (e) {}

    loadChartsData();
}

function updateDashboardStats(data) {
    const env = data.environment || {};
    document.getElementById('avg-ph').textContent = env.avg_ph ? env.avg_ph.toFixed(2) : '--';
    document.getElementById('avg-ca').textContent = env.avg_ca_ppm ? `${env.avg_ca_ppm.toFixed(1)} ppm` : '-- ppm';
    document.getElementById('avg-orp').textContent = env.avg_orp_mv ? `${env.avg_orp_mv.toFixed(0)} mV` : '-- mV';
    document.getElementById('active-alerts').textContent = data.alerts?.pending || 0;

    const ph = env.avg_ph || 7;
    const ca = env.avg_ca_ppm || 0;
    const phTrend = document.getElementById('avg-ph-trend');
    phTrend.textContent = ph < 5.5 ? '⚠ 偏低' : ph > 8 ? '偏高' : '正常';
    phTrend.className = 'trend ' + (ph < 5.5 ? 'danger' : ph > 8 ? 'warn' : 'ok');

    const caTrend = document.getElementById('avg-ca-trend');
    caTrend.textContent = ca > 200 ? '⚠ 过高' : ca > 150 ? '偏高' : '正常';
    caTrend.className = 'trend ' + (ca > 200 ? 'danger' : ca > 150 ? 'warn' : 'ok');

    const atRisk = (env.ph_alarm_count || 0) + (env.ca_alarm_count || 0);
    document.getElementById('at-risk').textContent = `${atRisk} 件受威胁`;

    const alertTrend = document.getElementById('alerts-trend');
    const p = data.alerts?.pending || 0;
    alertTrend.textContent = p > 0 ? `${p} 条待处理` : '所有正常';
    alertTrend.className = 'trend ' + (p > 5 ? 'danger' : p > 0 ? 'warn' : 'ok');
}

function updateEnvironmentCards(latestValues) {
    const phLowIds = latestValues.filter(r => r.sensor_type === 'pH' && r.value < THRESHOLDS.PH_LOW).map(r => r.sensor_id);
    const caHighIds = latestValues.filter(r => r.sensor_type === 'Ca2+' && r.value > THRESHOLDS.CA_HIGH).map(r => r.sensor_id);
    GLOBAL_STATE.alertSet = new Set([...phLowIds, ...caHighIds]);

    const map = document.getElementById('sensors-map');
    if (map) {
        const dots = map.querySelectorAll('.sensor-dot');
        dots.forEach(d => {
            const id = d.dataset.id;
            d.classList.toggle('alarm', GLOBAL_STATE.alertSet.has(id));
        });
    }
}

function renderRecentAlerts(alerts) {
    const container = document.getElementById('recent-alerts');
    if (!alerts || alerts.length === 0) {
        container.innerHTML = '<div class="empty-state">✅ 暂无告警，环境正常</div>';
        return;
    }
    container.innerHTML = alerts.slice(0, 6).map(a => `
        <div class="alert-item level-${a.alert_type === 'PH_LOW' || a.alert_type === 'CA_HIGH' ? 'HIGH' : 'MEDIUM'}">
            <div class="time">${new Date(a.created_at).toLocaleTimeString('zh-CN', {hour12:false})}</div>
            <div class="alert-body">
                <div class="alert-type">${alertTypeLabel(a.alert_type)} · ${a.sensor_id}</div>
                <div class="alert-msg">${a.message}</div>
            </div>
            <span class="badge">${a.status}</span>
        </div>
    `).join('');
}

function alertTypeLabel(t) {
    return { PH_LOW: 'pH过低', CA_HIGH: '钙离子过高', TEMP_HIGH: '温度过高', ORP_ABNORMAL: 'ORP异常', CORROSION_RISK: '腐蚀风险' }[t] || t;
}

function renderSensorMiniMap(sensors) {
    const map = document.getElementById('sensors-map');
    if (!map) return;
    map.innerHTML = '';
    const W = map.clientWidth || 600;
    const H = map.clientHeight || 280;
    const scaleX = W / 50, scaleY = H / 50;

    sensors.forEach(s => {
        const dot = document.createElement('div');
        const typeClass = s.sensor_type === 'pH' ? 'ph' : s.sensor_type === 'ORP' ? 'orp' : 'ca';
        dot.className = `sensor-dot ${typeClass}`;
        dot.dataset.id = s.id;
        dot.style.left = (s.grid_x * scaleX) + 'px';
        dot.style.top = ((50 - s.grid_y) * scaleY) + 'px';
        dot.title = `${s.id} (${s.sensor_type}) @ (${s.grid_x.toFixed(1)}, ${s.grid_y.toFixed(1)})`;

        dot.addEventListener('mouseenter', e => {
            let tip = document.getElementById('sensor-tooltip');
            if (!tip) {
                tip = document.createElement('div');
                tip.id = 'sensor-tooltip';
                tip.className = 'sensor-tooltip';
                map.appendChild(tip);
            }
            tip.textContent = `${s.id} [${s.sensor_type}] 坐标(${s.grid_x.toFixed(1)},${s.grid_y.toFixed(1)}) 深度${s.depth.toFixed(2)}m`;
            const rect = map.getBoundingClientRect();
            tip.style.left = (e.clientX - rect.left + 10) + 'px';
            tip.style.top = (e.clientY - rect.top + 10) + 'px';
            tip.style.display = 'block';
        });
        dot.addEventListener('mouseleave', () => {
            const tip = document.getElementById('sensor-tooltip');
            if (tip) tip.style.display = 'none';
        });
        map.appendChild(dot);
    });
}

function loadChartsData() {
    if (GLOBAL_STATE.charts.trend) updateTrendChart();
    if (GLOBAL_STATE.charts.corrosion) updateCorrosionChart();
}

function updateTrendChart() {
    const ctx = document.getElementById('trend-chart');
    if (!ctx) return;
    if (!GLOBAL_STATE.charts.trend) {
        const labels = Array.from({ length: 48 }, (_, i) => {
            const d = new Date(Date.now() - (47 - i) * 1800000);
            return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
        });
        const sensorId = document.getElementById('trend-sensor')?.value || 'PHR-001';
        const isPH = sensorId.startsWith('PHR');
        GLOBAL_STATE.charts.trend = new Chart(ctx, {
            type: 'line',
            data: {
                labels,
                datasets: [{
                    label: isPH ? 'pH 值' : sensorId.startsWith('ORP') ? 'ORP (mV)' : 'Ca²+ (ppm)',
                    data: generateMockHistory(48, sensorId),
                    borderColor: isPH ? '#4a9eff' : sensorId.startsWith('ORP') ? '#a78bfa' : '#26c6a0',
                    backgroundColor: isPH ? 'rgba(74,158,255,0.12)' : sensorId.startsWith('ORP') ? 'rgba(167,139,250,0.12)' : 'rgba(38,198,160,0.12)',
                    fill: true,
                    tension: 0.35,
                    borderWidth: 2,
                    pointRadius: 0,
                }]
            },
            options: chartBaseOptions({ yTitle: isPH ? 'pH' : sensorId.startsWith('ORP') ? 'mV' : 'ppm' })
        });
    } else {
        const sensorId = document.getElementById('trend-sensor')?.value || 'PHR-001';
        GLOBAL_STATE.charts.trend.data.datasets[0].data = generateMockHistory(48, sensorId);
        GLOBAL_STATE.charts.trend.update('none');
    }
    document.getElementById('trend-sensor')?.addEventListener('change', () => updateTrendChart());
}

function generateMockHistory(n, sensorId) {
    const isPH = sensorId.startsWith('PHR');
    const isORP = sensorId.startsWith('ORP');
    const base = isPH ? 6.5 : isORP ? 180 : 75;
    const amp = isPH ? 0.5 : isORP ? 80 : 20;
    const arr = [];
    let v = base;
    for (let i = 0; i < n; i++) {
        v += (Math.random() - 0.5) * amp * 0.3;
        v = base + (v - base) * 0.85 + Math.sin(i / 6) * amp * 0.4;
        arr.push(+(v.toFixed(isPH ? 2 : 1)));
    }
    return arr;
}

function updateCorrosionChart() {
    const ctx = document.getElementById('corrosion-chart');
    if (!ctx) return;
    const labels = Array.from({ length: 20 }, (_, i) => `R${i + 1}`);
    const data = labels.map(() => +(5 + Math.random() * 280).toFixed(1));
    const colors = data.map(v => {
        const rgb = sampleColorMap(v, 0, 300, 'CORROSION');
        return rgbToCss(rgb, 0.85);
    });
    if (!GLOBAL_STATE.charts.corrosion) {
        GLOBAL_STATE.charts.corrosion = new Chart(ctx, {
            type: 'bar',
            data: { labels, datasets: [{ label: '腐蚀深度 (μm)', data, backgroundColor: colors, borderRadius: 4, borderSkipped: false }] },
            options: chartBaseOptions({ yTitle: 'μm', isBar: true })
        });
    } else {
        GLOBAL_STATE.charts.corrosion.data.datasets[0].data = data;
        GLOBAL_STATE.charts.corrosion.data.datasets[0].backgroundColor = colors;
        GLOBAL_STATE.charts.corrosion.update('none');
    }
}

function chartBaseOptions({ yTitle, isBar = false }) {
    return {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
            legend: { labels: { color: '#8b98a5', font: { size: 10 }, boxWidth: 12 } },
            tooltip: {
                backgroundColor: '#243447',
                borderColor: '#2f4156',
                borderWidth: 1,
                titleColor: '#e6edf3',
                bodyColor: '#e6edf3',
                padding: 8,
            }
        },
        scales: {
            x: {
                ticks: { color: '#6e7681', font: { size: 9 }, maxRotation: 0, autoSkip: true, maxTicksLimit: 12 },
                grid: { color: 'rgba(255,255,255,0.04)' },
            },
            y: {
                title: { display: !!yTitle, text: yTitle, color: '#8b98a5', font: { size: 10 } },
                ticks: { color: '#6e7681', font: { size: 10 } },
                grid: { color: 'rgba(255,255,255,0.05)' },
                beginAtZero: isBar,
            }
        },
        animation: { duration: 500 },
    };
}

function bindDashboardInputs() {
    updateTrendChart();
    updateCorrosionChart();
}

function bindPointCloudControls() {
    document.getElementById('relic-select')?.addEventListener('change', e => {
        GLOBAL_STATE.currentRelicId = e.target.value;
        loadRelicPointCloud(e.target.value);
    });
    document.getElementById('color-mode')?.addEventListener('change', () => loadRelicPointCloud(GLOBAL_STATE.currentRelicId, true));
    document.getElementById('reset-view')?.addEventListener('click', () => BoneCloud.resetView());
}

function bindContourControls() {
    document.getElementById('contour-layer')?.addEventListener('change', renderContourView);
    document.getElementById('contour-window')?.addEventListener('change', renderContourView);
}

function bindAlertActions() {
    document.getElementById('chk-all')?.addEventListener('change', e => {
        document.querySelectorAll('.alert-row-chk').forEach(c => c.checked = e.target.checked);
    });
    document.getElementById('ack-all')?.addEventListener('click', () => batchAlertsAction('acknowledge'));
    document.getElementById('resolve-all')?.addEventListener('click', () => batchAlertsAction('resolve'));
}

async function batchAlertsAction(action) {
    const checks = Array.from(document.querySelectorAll('.alert-row-chk:checked'));
    for (const chk of checks) {
        await API.post(`/api/alerts/${chk.value}/action`, { action });
    }
    loadAlertsTable();
}

function onTabRelic() {
    BoneCloud.init('three-container');
    loadRelicPointCloud(GLOBAL_STATE.currentRelicId || GLOBAL_STATE.relics[0]?.id);
}

async function loadRelicPointCloud(relicId, useExistingData = false) {
    if (!relicId) return;
    try {
        const resp = await API.get(`/api/pointcloud/${relicId}`, { limit: 5000 });
        if (resp?.success && resp.data) {
            const colorMode = document.getElementById('color-mode')?.value || 'depth';
            const summary = BoneCloud.render(resp.data.points || [], colorMode);
            updateRelicDetail(relicId, summary);
            updateColorBar(colorMode);
        }
    } catch (e) {
        console.warn('点云加载失败:', e);
    }
}

function updateColorBar(mode) {
    const cbTitle = document.getElementById('colorbar-title');
    const labelsWrap = document.querySelector('.colorbar-labels');
    if (!cbTitle || !labelsWrap) return;
    const map = {
        depth: { title: '腐蚀深度 (μm)', labels: ['0 μm', '100', '200', '300 μm'] },
        collagen: { title: '胶原降解率 (%)', labels: ['0%', '33%', '66%', '100%'] },
        height: { title: '几何高度 Z', labels: ['Min', '', '', 'Max'] },
    };
    const info = map[mode] || map.depth;
    cbTitle.textContent = info.title;
    labelsWrap.innerHTML = info.labels.map(l => `<span>${l}</span>`).join('');
}

function updateRelicDetail(relicId, summary) {
    const relic = GLOBAL_STATE.relics.find(r => r.id === relicId);
    if (!relic) return;
    document.getElementById('d-id').textContent = relic.id;
    document.getElementById('d-name').textContent = `${relic.name} (${relic.category})`;
    document.getElementById('d-grid').textContent = `X=${relic.grid_x.toFixed(1)}, Y=${relic.grid_y.toFixed(1)}`;
    document.getElementById('d-depth').textContent = `${relic.burial_depth.toFixed(2)} m`;
    document.getElementById('d-date').textContent = relic.discovered_date;
    document.getElementById('d-cond').textContent = relic.initial_condition;

    if (summary) {
        document.getElementById('d-avg-depth').textContent = `${summary.avgDepth.toFixed(1)} μm`;
        document.getElementById('d-avg-depth').className = 'd-val ' + (summary.avgDepth > 100 ? 'val-danger' : summary.avgDepth > 40 ? 'val-warn' : '');
        document.getElementById('d-max-depth').textContent = `${summary.maxDepth.toFixed(1)} μm`;
        document.getElementById('d-max-depth').className = 'd-val ' + (summary.maxDepth > 200 ? 'val-danger' : summary.maxDepth > 80 ? 'val-warn' : '');
        document.getElementById('d-collagen').textContent = `${summary.avgCollagen.toFixed(1)}%`;
        const cap = 1.667 - (summary.avgCollagen / 100) * 0.3;
        document.getElementById('d-cap').textContent = cap.toFixed(3);
        const risk = corrosionRiskLevel(summary.avgDepth);
        const riskEl = document.getElementById('d-risk');
        riskEl.textContent = risk;
        riskEl.className = `d-val badge-risk ${risk}`;
    }
}

function getScatterForLayer(layer, hours = 24) {
    if (GLOBAL_STATE.latestValues && GLOBAL_STATE.latestValues.length > 0) {
        const byType = {};
        GLOBAL_STATE.latestValues.forEach(r => {
            const key = r.sensor_type === 'pH' ? 'pH' : r.sensor_type === 'ORP' ? 'orp' : r.sensor_type === 'Ca2+' ? 'ca' : 'temp';
            byType[key] = byType[key] || [];
            byType[key].push({ x: r.grid_x, y: r.grid_y, value: r.value, label: r.sensor_id });
        });
        if (layer === 'ph') return byType.pH || generateMockScatter('pH');
        if (layer === 'ca') return byType.ca || generateMockScatter('ca');
    }
    if (layer === 'depth') {
        return generateCorrosionScatter();
    }
    if (layer === 'collagen') {
        return generateCollagenScatter();
    }
    return generateMockScatter(layer);
}

function generateMockScatter(type) {
    const base = type === 'pH' ? 6.5 : type === 'ca' ? 75 : 200;
    const amp = type === 'pH' ? 1.5 : type === 'ca' ? 70 : 200;
    const out = [];
    const n = type === 'pH' ? 50 : type === 'ca' ? 30 : 50;
    for (let i = 0; i < n; i++) {
        out.push({
            x: 1 + Math.random() * 48,
            y: 1 + Math.random() * 48,
            value: +(base + (Math.random() - 0.3) * amp).toFixed(type === 'pH' ? 2 : 1),
            label: `S${i + 1}`,
        });
    }
    return out;
}

function generateCorrosionScatter() {
    const out = [];
    for (let i = 0; i < 200; i++) {
        const cx = 25, cy = 25;
        const x = Math.random() * 50;
        const y = Math.random() * 50;
        const d = Math.sqrt((x - cx) ** 2 + (y - cy) ** 2);
        const base = 280 * Math.exp(-d / 12) + 10;
        out.push({ x, y, value: +(base + (Math.random() - 0.5) * 50).toFixed(1) });
    }
    return out;
}

function generateCollagenScatter() {
    return generateCorrosionScatter().map(p => ({ ...p, value: +(p.value / 3).toFixed(1) }));
}

function renderContourView() {
    const layer = document.getElementById('contour-layer')?.value || 'ph';
    const hours = +(document.getElementById('contour-window')?.value || 24);
    const points = getScatterForLayer(layer, hours);
    const cfgMap = {
        ph: { colorMap: 'PH', numLevels: 10, valueFormatter: v => `pH${v.toFixed(1)}`, highlightY: 25 },
        ca: { colorMap: 'TURBO', numLevels: 10, valueFormatter: v => `${v.toFixed(0)}ppm`, highlightY: 25 },
        depth: { colorMap: 'CORROSION', numLevels: 10, valueFormatter: v => `${v.toFixed(0)}μm`, highlightY: 25 },
        collagen: { colorMap: 'PLASMA', numLevels: 8, valueFormatter: v => `${v.toFixed(0)}%`, highlightY: 25 },
    };
    const cfg = cfgMap[layer] || cfgMap.ph;
    const result = drawContourMap('contour-canvas', points, cfg);
    if (result) {
        renderSectionChart(result.grid, cfg.highlightY || 25, cfg.valueFormatter);
        renderRiskDistributionChart(points);
    }
}

function renderSectionChart(grid, yIdx, formatter) {
    const canvas = document.getElementById('section-chart');
    if (!canvas) return;
    const size = 50;
    const profile = extractProfile(grid, yIdx, size);
    const labels = Array.from({ length: size }, (_, i) => `${i}`);
    if (GLOBAL_STATE.charts.section) {
        GLOBAL_STATE.charts.section.data.labels = labels;
        GLOBAL_STATE.charts.section.data.datasets[0].data = profile;
        GLOBAL_STATE.charts.section.update('none');
    } else {
        GLOBAL_STATE.charts.section = new Chart(canvas, {
            type: 'line',
            data: {
                labels,
                datasets: [{
                    label: `Y=${yIdx} 剖面`,
                    data: profile,
                    borderColor: '#d4a855',
                    backgroundColor: 'rgba(212,168,85,0.15)',
                    fill: true,
                    tension: 0.3,
                    borderWidth: 2,
                    pointRadius: 0,
                }]
            },
            options: chartBaseOptions({ yTitle: '数值' })
        });
    }
}

function renderRiskDistributionChart(points) {
    const canvas = document.getElementById('risk-chart');
    if (!canvas) return;
    const counts = { LOW: 0, MEDIUM: 0, HIGH: 0, CRITICAL: 0 };
    points.forEach(p => {
        const v = p.value;
        let level = 'LOW';
        if (v > 200 || (typeof p.value === 'number' && p.value < 5.5 && p.value > 0 && p.value < 14)) level = 'HIGH';
        else if (v > 150 || (typeof p.value === 'number' && p.value < 6 && p.value > 0)) level = 'MEDIUM';
        else if (v > 300 || (typeof p.value === 'number' && p.value < 4.5)) level = 'CRITICAL';
        if (typeof p.value === 'number' && p.value > 0 && p.value < 14) {
            if (p.value < 5.5) level = 'HIGH';
            else if (p.value < 6) level = 'MEDIUM';
            else level = 'LOW';
        } else {
            if (p.value > 200) level = 'CRITICAL';
            else if (p.value > 150) level = 'HIGH';
            else if (p.value > 80) level = 'MEDIUM';
            else level = 'LOW';
        }
        counts[level]++;
    });
    const labels = ['LOW', 'MEDIUM', 'HIGH', 'CRITICAL'];
    const colors = ['#3fb950', '#ffa657', '#f45a5a', '#b91c1c'];
    if (GLOBAL_STATE.charts.risk) {
        GLOBAL_STATE.charts.risk.data.datasets[0].data = labels.map(l => counts[l]);
        GLOBAL_STATE.charts.risk.update('none');
    } else {
        GLOBAL_STATE.charts.risk = new Chart(canvas, {
            type: 'doughnut',
            data: {
                labels,
                datasets: [{
                    data: labels.map(l => counts[l]),
                    backgroundColor: colors.map(c => c + 'cc'),
                    borderColor: '#1e2a38',
                    borderWidth: 2,
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                plugins: {
                    legend: { position: 'right', labels: { color: '#8b98a5', font: { size: 10 }, boxWidth: 12 } }
                }
            }
        });
    }
}

function renderGridView() {
    const phPoints = getScatterForLayer('ph');
    const caPoints = getScatterForLayer('ca');
    drawGridHeatmap('grid-ph-canvas', phPoints, 'PH', v => `pH${v.toFixed(1)}`);
    drawGridHeatmap('grid-ca-canvas', caPoints, 'TURBO', v => `${v.toFixed(0)}ppm`);
}

function renderAnalysisView() {
    bindAnalysisInputs();
    calculateArrhenius();
    if (!GLOBAL_STATE.charts.arrhenius) renderArrheniusChart();
    if (!GLOBAL_STATE.charts.cap) renderCapChart();
    updateArrheniusChart();
    updateCapChart();
}

function bindAnalysisInputs() {
    const inputs = ['temp', 'ph', 'orp', 'months'];
    inputs.forEach(k => {
        const el = document.getElementById(`inp-${k}`);
        const out = document.getElementById(`val-${k}`);
        if (!el) return;
        el.addEventListener('input', () => {
            const unit = { temp: ' ℃', ph: '', orp: ' mV', months: ' 月' }[k];
            out.textContent = el.value + unit;
            calculateArrhenius();
            updateArrheniusChart();
            updateCapChart();
        });
    });
}

function calculateArrhenius() {
    const T = +document.getElementById('inp-temp').value;
    const pH = +document.getElementById('inp-ph').value;
    const Eh = +document.getElementById('inp-orp').value;
    const months = +document.getElementById('inp-months').value;

    const R = 8.314;
    const Ea = 85000;
    const A = 1.2e10;
    const Tk = T + 273.15;
    const kArr = A * Math.exp(-Ea / (R * Tk));
    const hPlus = Math.pow(10, -pH);
    const ohMinus = Math.pow(10, pH - 14);
    const phFactor = 1 + 4.5e-4 * hPlus + 8e-5 * ohMinus;
    const orpFactor = 1 + 0.8 * Math.max(0, Math.min(1, (Eh + 300) / 600));
    const k = kArr * phFactor * orpFactor;
    const seconds = months * 30 * 24 * 3600;
    const degPct = (1 - Math.exp(-k * seconds)) * 100;
    const corRate = (k * 1e7 + 0.5) * (pH < 6 ? (6 - pH) * 5 + 1 : 1) * 0.1;
    const corDepth = corRate * (months / 12);
    const days = months * 30;
    const cap = 1.667;
    let caPred = cap;
    if (pH < 7) {
        caPred = cap * (1 + (7 - pH) * 0.08 * Math.log10(Math.max(days, 1) / 10 + 1));
    }

    document.getElementById('out-k').textContent = k.toExponential(2);
    document.getElementById('out-deg').textContent = Math.min(100, degPct).toFixed(2);
    document.getElementById('out-rate').textContent = corRate.toFixed(2);
    document.getElementById('out-depth').textContent = corDepth.toFixed(1);
    document.getElementById('out-cap').textContent = caPred.toFixed(3);
}

function renderArrheniusChart() {
    const ctx = document.getElementById('arrhenius-chart');
    if (!ctx) return;
    const temps = Array.from({ length: 50 }, (_, i) => i - 5);
    GLOBAL_STATE.charts.arrhenius = new Chart(ctx, {
        type: 'line',
        data: { labels: temps.map(t => `${t}℃`), datasets: [] },
        options: {
            ...chartBaseOptions({ yTitle: 'k (s⁻¹)' }),
            scales: {
                x: { title: { display: true, text: '温度', color: '#8b98a5' }, ticks: { color: '#6e7681', font: { size: 9 } }, grid: { color: 'rgba(255,255,255,0.04)' } },
                y: { type: 'logarithmic', title: { display: true, text: '速率常数 k (log)', color: '#8b98a5' }, ticks: { color: '#6e7681', font: { size: 9 } }, grid: { color: 'rgba(255,255,255,0.05)' } }
            }
        }
    });
}

function updateArrheniusChart() {
    const chart = GLOBAL_STATE.charts.arrhenius;
    if (!chart) return;
    const pH = +document.getElementById('inp-ph').value;
    const Eh = +document.getElementById('inp-orp').value;
    const R = 8.314, Ea = 85000, A = 1.2e10;
    const temps = Array.from({ length: 50 }, (_, i) => i - 5);
    const neutral = temps.map(T => {
        const kArr = A * Math.exp(-Ea / (R * (T + 273.15)));
        return kArr;
    });
    const withPH = temps.map(T => {
        const kArr = A * Math.exp(-Ea / (R * (T + 273.15)));
        const hPlus = Math.pow(10, -pH);
        const ohMinus = Math.pow(10, pH - 14);
        return kArr * (1 + 4.5e-4 * hPlus + 8e-5 * ohMinus);
    });
    const full = temps.map(T => {
        const kArr = A * Math.exp(-Ea / (R * (T + 273.15)));
        const hPlus = Math.pow(10, -pH);
        const ohMinus = Math.pow(10, pH - 14);
        const f1 = 1 + 4.5e-4 * hPlus + 8e-5 * ohMinus;
        const f2 = 1 + 0.8 * Math.max(0, Math.min(1, (Eh + 300) / 600));
        return kArr * f1 * f2;
    });
    chart.data.datasets = [
        { label: '仅温度 (Arrhenius)', data: neutral, borderColor: '#4a9eff', backgroundColor: 'transparent', tension: 0.4, borderWidth: 2, pointRadius: 0 },
        { label: `+ pH修正 (pH=${pH})`, data: withPH, borderColor: '#26c6a0', backgroundColor: 'transparent', tension: 0.4, borderWidth: 2, pointRadius: 0 },
        { label: `+ ORP修正 (Eh=${Eh}mV)`, data: full, borderColor: '#d4a855', backgroundColor: 'rgba(212,168,85,0.1)', fill: true, tension: 0.4, borderWidth: 2.5, pointRadius: 0 },
    ];
    chart.update('none');
}

function renderCapChart() {
    const ctx = document.getElementById('cap-chart');
    if (!ctx) return;
    const days = Array.from({ length: 30 }, (_, i) => (i + 1) * 10);
    GLOBAL_STATE.charts.cap = new Chart(ctx, {
        type: 'line',
        data: { labels: days.map(d => `第${d}天`), datasets: [] },
        options: chartBaseOptions({ yTitle: 'Ca/P比' })
    });
}

function updateCapChart() {
    const chart = GLOBAL_STATE.charts.cap;
    if (!chart) return;
    const pHValues = [4.5, 5.5, 6.5, 7.5, 8.5];
    const colors = ['#f45a5a', '#ffa657', '#e8b84a', '#26c6a0', '#4a9eff'];
    const T = +document.getElementById('inp-temp').value;
    const tempFactor = Math.exp(-85000 / 8.314 * (1 / (T + 273.15) - 1 / 298.15));
    const datasets = pHValues.map((pH, idx) => {
        const data = Array.from({ length: 30 }, (_, i) => {
            const d = (i + 1) * 10;
            const acidFactor = pH < 7 ? Math.exp((7 - pH) * 0.35) : 1 / Math.exp((pH - 7) * 0.1);
            const dissolution = (1 - Math.exp(-d / 200 * tempFactor * acidFactor)) * 1.5;
            return +(1.667 * (1 + dissolution * 0.15)).toFixed(3);
        });
        return {
            label: `pH ${pH}`, data,
            borderColor: colors[idx], backgroundColor: 'transparent',
            tension: 0.4, borderWidth: 2, pointRadius: 0,
        };
    });
    datasets.push({
        label: '化学计量比 1.667',
        data: Array(30).fill(1.667),
        borderColor: '#8b98a5',
        borderDash: [5, 5],
        backgroundColor: 'transparent',
        borderWidth: 1.5,
        pointRadius: 0,
    });
    chart.data.datasets = datasets;
    chart.update('none');
}

async function loadAlertsTable() {
    try {
        const resp = await API.get('/api/alerts', { limit: 100 });
        if (!resp?.success || !resp.data) return;
        const tbody = document.getElementById('alert-tbody');
        if (!tbody) return;
        const alerts = resp.data.alerts || [];
        if (alerts.length === 0) {
            tbody.innerHTML = '<tr><td colspan="11" class="empty-state">暂无告警记录</td></tr>';
            document.getElementById('alert-summary').textContent = '总计: 0 | 未处理: 0';
            return;
        }
        tbody.innerHTML = alerts.map(a => `
            <tr>
                <td><input type="checkbox" class="alert-row-chk" value="${a.id}"/></td>
                <td class="mono">${a.id}</td>
                <td><span class="type-tag ${a.alert_type}">${alertTypeLabel(a.alert_type)}</span></td>
                <td>${a.sensor_id}</td>
                <td>${a.threshold.toFixed(2)}</td>
                <td style="color:${Math.abs(a.actual_value - a.threshold) > Math.abs(a.threshold) * 0.1 ? '#f45a5a' : '#ffa657'};font-weight:600">${a.actual_value.toFixed(2)}</td>
                <td style="max-width:320px">${a.message}</td>
                <td>${(a.channels || []).join(',')}</td>
                <td class="mono">${new Date(a.created_at).toLocaleString('zh-CN', { hour12: false })}</td>
                <td><span class="status-tag ${a.status}">${a.status}</span></td>
                <td>
                    <button class="op-btn" onclick="alertAction('${a.id}','acknowledge')">确认</button>
                    <button class="op-btn" onclick="alertAction('${a.id}','resolve')">恢复</button>
                </td>
            </tr>
        `).join('');
        const s = resp.data.stats || {};
        document.getElementById('alert-summary').textContent = `总计: ${s.total || 0} | 未处理: ${(s.pending || 0) + (s.sent || 0)}`;
    } catch (e) {}
}

async function alertAction(id, action) {
    await API.post(`/api/alerts/${id}/action`, { action });
    loadAlertsTable();
}

async function loadSensorsTable() {
    try {
        const sensors = GLOBAL_STATE.sensors.length ? GLOBAL_STATE.sensors : (await API.get('/api/sensors'))?.data || [];
        const latest = GLOBAL_STATE.latestValues.length ? GLOBAL_STATE.latestValues : (await API.get('/api/sensors/latest'))?.data || [];
        const tbody = document.getElementById('sensor-tbody');
        if (!tbody) return;
        const latestMap = Object.fromEntries(latest.map(r => [r.sensor_id, r]));
        tbody.innerHTML = sensors.slice(0, 200).map(s => {
            const r = latestMap[s.id];
            const val = r ? `${r.value.toFixed(s.sensor_type === 'pH' ? 2 : 1)}${s.sensor_type === 'pH' ? '' : s.sensor_type === 'ORP' ? ' mV' : ' ppm'}` : '--';
            const isAlarm = r && ((s.sensor_type === 'pH' && r.value < 5.5) || (s.sensor_type === 'Ca2+' && r.value > 200));
            return `<tr>
                <td>${s.id}</td>
                <td>${s.sensor_type}</td>
                <td>(${s.grid_x.toFixed(1)}, ${s.grid_y.toFixed(1)})</td>
                <td>${s.depth.toFixed(2)} m</td>
                <td>${s.install_date}</td>
                <td style="color:${isAlarm ? '#f45a5a' : '#26c6a0'};font-weight:600">${val}</td>
                <td><span class="status-tag ${isAlarm ? 'FAILED' : 'RESOLVED'}">${isAlarm ? '告警' : s.status}</span></td>
            </tr>`;
        }).join('');
    } catch (e) {}
}

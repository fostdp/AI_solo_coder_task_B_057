const API_BASE = window.location.origin;

let ehphChart = null;
let cpiChart = null;
let excavChart = null;
let excavDistChart = null;

const ZONE_COLORS = {
    'OXIDIZED': '#8B0000',
    'SUBSURFACE_OXIC': '#FF4500',
    'MANGANESE_REDUCING': '#FFA500',
    'IRON_REDUCING': '#9ACD32',
    'SULFATE_REDUCING': '#00CED1',
    'METHANOGENIC': '#4169E1',
    'CARBONATE_REDUCING': '#483D8B',
    'UNDEFINED': '#696969'
};

function riskBadgeClass(risk) {
    const map = { 'LOW': 'LOW', 'MEDIUM': 'MEDIUM', 'HIGH': 'HIGH', 'CRITICAL': 'CRITICAL' };
    return map[risk] || 'LOW';
}

async function postJSON(url, data) {
    const resp = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data)
    });
    return resp.json();
}

function initEhPh() {
    const btn = document.getElementById('btn-calc-ehph');
    if (btn) btn.addEventListener('click', calcEhPhDiagram);
}

async function calcEhPhDiagram() {
    const ph = parseFloat(document.getElementById('ehph-inp-ph').value);
    const eh = parseFloat(document.getElementById('ehph-inp-eh').value);

    try {
        const resp = await postJSON(`${API_BASE}/api/heritage/eh-ph-diagram`, {
            ph, eh_mv: eh, ph_min: 2.0, ph_max: 12.0, eh_min: -500.0, eh_max: 800.0, grid_x: 20, grid_y: 20
        });

        if (resp.success && resp.data) {
            renderEhPhCanvas(resp.data);
            renderEhPhDetails(resp.data);
        }
    } catch (e) {
        alert('Eh-pH相图计算失败: ' + e.message);
    }
}

function renderEhPhCanvas(data) {
    const canvas = document.getElementById('ehph-canvas');
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const W = canvas.width, H = canvas.height;
    const pad = { l: 60, r: 30, t: 30, b: 50 };
    const plotW = W - pad.l - pad.r, plotH = H - pad.t - pad.b;

    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, W, H);

    const phMin = 2.0, phMax = 12.0;
    const ehMin = -500.0, ehMax = 800.0;

    if (data.zones && data.zones.length > 0) {
        const nx = data.grid_size[0], ny = data.grid_size[1];
        const cellW = plotW / (nx - 1);
        const cellH = plotH / (ny - 1);

        for (let i = 0; i < nx; i++) {
            for (let j = 0; j < ny; j++) {
                const idx = i * ny + j;
                const zone = data.zones[idx];
                if (!zone) continue;
                const color = ZONE_COLORS[zone.zone] || '#ccc';
                const x = pad.l + i * cellW - cellW / 2;
                const y = pad.t + (ny - 1 - j) * cellH - cellH / 2;
                ctx.fillStyle = color;
                ctx.globalAlpha = 0.75;
                ctx.fillRect(x, y, cellW + 1, cellH + 1);
            }
        }
        ctx.globalAlpha = 1.0;
    }

    if (data.boundaries) {
        const colors = ['#ff0000', '#ff6600', '#cc9900', '#006600', '#0066cc', '#6600cc', '#990000'];
        data.boundaries.forEach((b, bi) => {
            if (!b.boundary_line) return;
            ctx.strokeStyle = colors[bi % colors.length];
            ctx.lineWidth = 2;
            ctx.setLineDash(bi % 2 === 0 ? [] : [5, 4]);
            ctx.beginPath();
            b.boundary_line.forEach((pt, k) => {
                const x = pad.l + ((pt[0] - phMin) / (phMax - phMin)) * plotW;
                const y = pad.t + (1 - (pt[1] - ehMin) / (ehMax - ehMin)) * plotH;
                if (k === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
            });
            ctx.stroke();
        });
        ctx.setLineDash([]);
    }

    ctx.strokeStyle = '#333';
    ctx.lineWidth = 1.5;
    ctx.strokeRect(pad.l, pad.t, plotW, plotH);

    ctx.fillStyle = '#333';
    ctx.font = '12px sans-serif';
    ctx.textAlign = 'center';
    for (let p = 2; p <= 12; p += 2) {
        const x = pad.l + ((p - phMin) / (phMax - phMin)) * plotW;
        ctx.fillText(p.toString(), x, H - pad.b + 18);
        ctx.beginPath();
        ctx.moveTo(x, pad.t);
        ctx.lineTo(x, pad.t + plotH);
        ctx.strokeStyle = '#eee';
        ctx.stroke();
    }

    ctx.textAlign = 'right';
    for (let e = -400; e <= 800; e += 200) {
        const y = pad.t + (1 - (e - ehMin) / (ehMax - ehMin)) * plotH;
        ctx.fillText(e + ' mV', pad.l - 8, y + 4);
        ctx.beginPath();
        ctx.moveTo(pad.l, y);
        ctx.lineTo(pad.l + plotW, y);
        ctx.strokeStyle = '#eee';
        ctx.stroke();
    }
    ctx.strokeStyle = '#333';

    ctx.fillStyle = '#000';
    ctx.font = 'bold 13px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('pH 值', W / 2, H - 10);
    ctx.save();
    ctx.translate(16, H / 2);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText('Eh (mV)', 0, 0);
    ctx.restore();

    if (data.sample_point) {
        const sx = pad.l + ((data.sample_point.ph - phMin) / (phMax - phMin)) * plotW;
        const sy = pad.t + (1 - (data.sample_point.eh_mv - ehMin) / (ehMax - ehMin)) * plotH;
        ctx.fillStyle = '#ff0000';
        ctx.beginPath();
        ctx.arc(sx, sy, 8, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = '#fff';
        ctx.lineWidth = 2;
        ctx.stroke();
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 10px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText('★', sx, sy + 4);
    }
}

function renderEhPhDetails(data) {
    const sp = data.sample_point;
    document.getElementById('ehph-sample').textContent = `pH=${sp.ph.toFixed(2)}, Eh=${sp.eh_mv.toFixed(0)}mV`;
    const zoneEl = document.getElementById('ehph-zone');
    zoneEl.textContent = data.dominant_zone_name;
    zoneEl.className = 'd-val badge-risk ' + riskBadgeClass(data.corrosion_risk);
    document.getElementById('ehph-phase').textContent = sp.stable_phase;
    document.getElementById('ehph-quality').textContent = data.preservation_quality;
    const riskEl = document.getElementById('ehph-risk');
    riskEl.textContent = data.corrosion_risk;
    riskEl.className = 'd-val badge-risk ' + riskBadgeClass(data.corrosion_risk);

    const bndEl = document.getElementById('ehph-boundaries');
    if (data.boundaries && data.boundaries.length > 0) {
        bndEl.innerHTML = data.boundaries.map(b => `
            <div class="boundary-item">
                <div class="b-title">${b.reaction}</div>
                <div class="b-eq"><code>${b.equation}</code></div>
                <div class="b-desc">${b.description}</div>
            </div>
        `).join('');
    }
}

function initCpi() {
    const btn = document.getElementById('btn-calc-cpi');
    if (btn) btn.addEventListener('click', calcCpi);
}

window.addTempHistoryRow = function () {
    const tbody = document.getElementById('cpi-temp-history');
    if (!tbody) return;
    const tr = document.createElement('tr');
    tr.innerHTML = `<td><input type="number" class="mini-input" step="100" placeholder="年"/></td>
        <td><input type="number" class="mini-input" step="0.5" placeholder="℃"/></td>
        <td><button class="mini-btn" onclick="this.closest('tr').remove()">删除</button></td>`;
    tbody.appendChild(tr);
};

async function calcCpi() {
    const burialYears = parseFloat(document.getElementById('cpi-years').value);
    const temp = parseFloat(document.getElementById('cpi-temp').value);
    const ea = parseFloat(document.getElementById('cpi-ea').value) * 1000;
    const initFrac = parseFloat(document.getElementById('cpi-init').value);

    const history = [];
    document.querySelectorAll('#cpi-temp-history tr').forEach(tr => {
        const inputs = tr.querySelectorAll('input.mini-input');
        if (inputs.length >= 2) {
            const y = parseFloat(inputs[0].value);
            const t = parseFloat(inputs[1].value);
            if (!isNaN(y) && !isNaN(t)) history.push({ years_bp: y, temp_celsius: t });
        }
    });

    try {
        const resp = await postJSON(`${API_BASE}/api/heritage/collagen-preservation-index`, {
            activation_energy: ea,
            burial_years: burialYears,
            current_temp_c: temp,
            temperature_history: history.length > 0 ? history : null,
            initial_collagen_fraction: initFrac
        });

        if (resp.success && resp.data) renderCpiResult(resp.data);
    } catch (e) {
        alert('CPI计算失败: ' + e.message);
    }
}

function renderCpiResult(data) {
    document.getElementById('cpi-score').textContent = data.cpi_score.toFixed(1);
    document.getElementById('cpi-grade').textContent = data.cpi_grade;
    document.getElementById('cpi-eq-yrs').textContent = data.equivalent_years_at_20c.toFixed(0);
    document.getElementById('cpi-remaining').textContent = data.remaining_collagen_pct.toFixed(1);
    document.getElementById('cpi-hl').textContent = data.predicted_half_life_years.toFixed(0);
    document.getElementById('cpi-hl-ref').textContent = data.initial_half_life_years.toFixed(0);
    document.getElementById('cpi-avg-t').textContent = data.average_temp_c.toFixed(1);
    document.getElementById('cpi-interp').textContent = data.interpretation;

    const labels = [];
    const tempData = [];
    if (data.temperature_history) {
        data.temperature_history.slice().reverse().forEach(p => {
            labels.push(p.years_bp.toFixed(0) + '年前');
            tempData.push(p.temp_celsius);
        });
    }

    const cpiCanvas = document.getElementById('cpi-chart');
    if (cpiCanvas) {
        if (cpiChart) cpiChart.destroy();
        cpiChart = new Chart(cpiCanvas, {
            type: 'bar',
            data: {
                labels: labels.length > 0 ? labels : ['初始', '埋藏中期', '现代'],
                datasets: [
                    {
                        type: 'line',
                        label: '埋藏温度 (℃)',
                        data: tempData.length > 0 ? tempData : [8, 10, 12],
                        borderColor: '#ff6b6b',
                        backgroundColor: 'rgba(255,107,107,0.1)',
                        fill: true,
                        tension: 0.4,
                        yAxisID: 'y'
                    },
                    {
                        label: '剩余胶原 (%)',
                        data: [100, data.remaining_collagen_pct * 1.3, data.remaining_collagen_pct],
                        backgroundColor: '#4ecdc4',
                        yAxisID: 'y1'
                    }
                ]
            },
            options: {
                responsive: true,
                title: { display: true, text: '温度史与胶原剩余量趋势' },
                scales: {
                    y: { position: 'left', title: { display: true, text: '温度 (℃)' } },
                    y1: { position: 'right', title: { display: true, text: '胶原剩余 (%)' }, grid: { drawOnChartArea: false } }
                }
            }
        });
    }
}

function initExcavation() {
    const btn = document.getElementById('btn-calc-excav');
    if (btn) btn.addEventListener('click', calcExcavation);
}

async function calcExcavation() {
    const params = {
        num_simulations: parseInt(document.getElementById('excv-sims').value),
        current_ph: parseFloat(document.getElementById('excv-ph').value),
        ph_std_dev: parseFloat(document.getElementById('excv-phsd').value),
        current_temp_c: parseFloat(document.getElementById('excv-t').value),
        temp_std_dev: parseFloat(document.getElementById('excv-tsd').value),
        current_ca_ppm: parseFloat(document.getElementById('excv-ca').value),
        ca_std_dev: 15.0,
        current_orp_mv: parseFloat(document.getElementById('excv-orp').value),
        orp_std_dev: 50.0,
        forecast_years: parseFloat(document.getElementById('excv-years').value),
        time_steps_per_year: 12,
        target_corrosion_threshold_um: parseFloat(document.getElementById('excv-thr').value),
        acceptable_risk_threshold: parseFloat(document.getElementById('excv-risk').value),
        current_collagen_remaining_pct: 70.0
    };

    try {
        const resp = await postJSON(`${API_BASE}/api/heritage/excavation-optimization`, { params });
        if (resp.success && resp.data) renderExcavationResult(resp.data);
    } catch (e) {
        alert('蒙特卡洛模拟失败: ' + e.message);
    }
}

function renderExcavationResult(data) {
    document.getElementById('excv-done').textContent = data.simulations_completed + ' 次';
    document.getElementById('excv-conf').textContent = (data.confidence_level * 100).toFixed(0) + '%';
    document.getElementById('excv-window').textContent =
        `${data.optimal_window.start_year.toFixed(1)} ~ ${data.optimal_window.end_year.toFixed(1)} 年`;
    document.getElementById('excv-prob').textContent =
        `成功概率: ${(data.optimal_window.probability_of_success * 100).toFixed(0)}% | 净收益: ${data.optimal_window.net_benefit.toFixed(1)}μm`;
    document.getElementById('excv-rec').textContent = data.final_recommendation;

    const winEl = document.getElementById('excv-windows');
    if (data.windows) {
        winEl.innerHTML = data.windows.map(w => {
            const barWidth = Math.min(100, w.probability_of_success * 100);
            const barColor = w.probability_of_success >= 0.8 ? '#28a745'
                : w.probability_of_success >= 0.6 ? '#ffc107' : '#dc3545';
            return `<div class="window-item">
                <div class="w-header">
                    <span class="w-range">${w.start_year.toFixed(1)} - ${w.end_year.toFixed(1)}年</span>
                    <span class="w-rec">${w.recommendation}</span>
                </div>
                <div class="w-bar" style="background:#eee;border-radius:4px;height:10px;">
                    <div style="width:${barWidth}%;height:100%;background:${barColor};border-radius:4px;"></div>
                </div>
                <div class="w-meta">成功 ${(w.probability_of_success * 100).toFixed(0)}% |
                    等损 ${w.expected_damage_if_wait.toFixed(1)}μm vs 发掘 ${w.expected_damage_if_excavate.toFixed(1)}μm |
                    净收益 ${w.net_benefit >= 0 ? '+' : ''}${w.net_benefit.toFixed(1)}μm</div>
            </div>`;
        }).join('');
    }

    const stats = data.year_by_year_stats || [];
    const labels = stats.map(s => s.year.toFixed(0) + '年');
    const meanData = stats.map(s => s.mean_corrosion_um);
    const p5Data = stats.map(s => s.p5_corrosion_um);
    const p95Data = stats.map(s => s.p95_corrosion_um);
    const probData = stats.map(s => s.prob_exceed_threshold * 100);
    const threshold = data.params.target_corrosion_threshold_um;

    const excvCanvas = document.getElementById('excv-chart');
    if (excvCanvas) {
        if (excavChart) excavChart.destroy();
        excavChart = new Chart(excvCanvas, {
            type: 'line',
            data: {
                labels,
                datasets: [
                    { label: 'P95 (悲观)', data: p95Data, borderColor: '#dc3545', borderDash: [5, 5], fill: false, tension: 0.3 },
                    { label: '均值腐蚀 (μm)', data: meanData, borderColor: '#007bff', backgroundColor: 'rgba(0,123,255,0.15)', fill: '+1', tension: 0.3 },
                    { label: 'P5 (乐观)', data: p5Data, borderColor: '#28a745', borderDash: [5, 5], fill: false, tension: 0.3 },
                    { label: `阈值 ${threshold}μm`, data: labels.map(() => threshold), borderColor: '#ff6b00', borderWidth: 2, borderDash: [10, 4], fill: false, pointRadius: 0 }
                ]
            },
            options: {
                responsive: true,
                title: { display: true, text: '腐蚀深度蒙特卡洛预测 (置信区间)' },
                scales: { y: { title: { display: true, text: '腐蚀深度 (μm)' } } }
            }
        });
    }

    const distCanvas = document.getElementById('excv-dist-chart');
    if (distCanvas) {
        if (excavDistChart) excavDistChart.destroy();
        excavDistChart = new Chart(distCanvas, {
            type: 'bar',
            data: {
                labels,
                datasets: [
                    { label: '超阈概率 (%)', data: probData, backgroundColor: probData.map(p => p >= 30 ? '#dc3545' : p >= 15 ? '#ffc107' : '#28a745') }
                ]
            },
            options: {
                responsive: true,
                title: { display: true, text: '逐年超阈概率' },
                scales: { y: { title: { display: true, text: '概率 (%)' }, max: 100 } }
            }
        });
    }
}

function initProtection() {
    const btn = document.getElementById('btn-calc-prot');
    if (btn) btn.addEventListener('click', calcProtection);
}

async function calcProtection() {
    const payload = {
        ph: parseFloat(document.getElementById('prot-ph').value),
        ca_ppm: parseFloat(document.getElementById('prot-ca').value),
        orp_mv: parseFloat(document.getElementById('prot-orp').value),
        ambient_temp_c: parseFloat(document.getElementById('prot-temp').value),
        ambient_rh_pct: parseFloat(document.getElementById('prot-rh').value),
        burial_depth_m: parseFloat(document.getElementById('prot-depth').value),
        relic_category: document.getElementById('prot-category').value
    };
    try {
        const resp = await postJSON(`${API_BASE}/api/heritage/temporary-protection`, payload);
        if (resp.success && resp.data) renderProtectionResult(resp.data);
    } catch (e) {
        alert('保护方案生成失败: ' + e.message);
    }
}

function renderProtectionResult(data) {
    document.getElementById('prot-main').textContent = data.primary_moisturizer_zh;
    document.getElementById('prot-conc').textContent = data.concentration_pct.toFixed(0) + '%';
    document.getElementById('prot-method').textContent = data.application_method;
    const scoreEl = document.getElementById('prot-score');
    const cls = data.expected_effectiveness_score >= 85 ? 'LOW'
        : data.expected_effectiveness_score >= 75 ? 'MEDIUM' : 'HIGH';
    scoreEl.textContent = data.expected_effectiveness_score.toFixed(0) + ' 分';
    scoreEl.className = 'd-val badge-risk ' + cls;
    document.getElementById('prot-neutralize').textContent =
        data.ph_neutralization_required
            ? `需要 (${data.neutralization_agent || ''})` : '不需要';
    document.getElementById('prot-stab').textContent = data.estimated_stabilization_hours + ' 小时';

    const warnEl = document.getElementById('prot-warnings');
    if (data.warnings && data.warnings.length > 0) {
        warnEl.innerHTML = data.warnings.map(w => `<div class="warn-item">${w}</div>`).join('');
    } else {
        warnEl.innerHTML = '<div class="empty-state">无特殊警告</div>';
    }

    const matEl = document.getElementById('prot-materials');
    if (data.materials_needed) {
        matEl.innerHTML = data.materials_needed.map(m => {
            const pr = m.priority === '必要' ? 'color:#dc3545;font-weight:bold' : 'color:#28a745';
            return `<div class="mat-item">
                <div class="mat-head">
                    <span class="mat-name">${m.name_zh}</span>
                    <span class="mat-prio" style="${pr}">[${m.priority}]</span>
                </div>
                <div class="mat-body">
                    <div><b>用量:</b> ${m.quantity_estimate}</div>
                    <div><b>用途:</b> ${m.purpose}</div>
                </div>
            </div>`;
        }).join('');
    }

    const protEl = document.getElementById('prot-protocol');
    if (data.step_by_step_protocol) {
        protEl.innerHTML = data.step_by_step_protocol.map(s => `<li>${s}</li>`).join('');
    }

    const secEl = document.getElementById('prot-secondary');
    if (data.secondary_recommendations && data.secondary_recommendations.length > 0) {
        secEl.innerHTML = data.secondary_recommendations.map(s => `<div class="sec-item">✓ ${s}</div>`).join('');
    } else {
        secEl.innerHTML = '<div class="empty-state">无需辅助措施</div>';
    }

    const dpEl = document.getElementById('prot-decision-path');
    if (data.decision_path) {
        dpEl.innerHTML = data.decision_path.map((step, i) => `
            <div class="dp-step">
                <span class="dp-num">${i + 1}</span>
                <span class="dp-text">${step}</span>
            </div>
        `).join('');
    }
}

document.addEventListener('DOMContentLoaded', () => {
    initEhPh();
    initCpi();
    initExcavation();
    initProtection();
});

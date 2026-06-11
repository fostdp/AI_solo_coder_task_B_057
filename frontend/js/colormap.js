const COLOR_MAPS = {
    VIRIDIS: [
        [0.267, 0.004, 0.329],
        [0.282, 0.140, 0.458],
        [0.253, 0.265, 0.530],
        [0.206, 0.371, 0.553],
        [0.163, 0.471, 0.558],
        [0.127, 0.566, 0.550],
        [0.134, 0.658, 0.517],
        [0.266, 0.752, 0.440],
        [0.477, 0.821, 0.318],
        [0.741, 0.873, 0.150],
        [0.993, 0.906, 0.143],
    ],
    PLASMA: [
        [0.050, 0.029, 0.528],
        [0.188, 0.028, 0.662],
        [0.326, 0.005, 0.722],
        [0.458, 0.026, 0.707],
        [0.579, 0.097, 0.630],
        [0.687, 0.187, 0.521],
        [0.782, 0.288, 0.401],
        [0.862, 0.398, 0.281],
        [0.925, 0.519, 0.166],
        [0.970, 0.654, 0.054],
        [0.993, 0.798, 0.143],
    ],
    TURBO: [
        [0.189, 0.071, 0.232],
        [0.214, 0.375, 0.945],
        [0.103, 0.667, 0.945],
        [0.094, 0.906, 0.812],
        [0.243, 0.988, 0.584],
        [0.529, 0.984, 0.305],
        [0.768, 0.929, 0.202],
        [0.952, 0.792, 0.180],
        [0.995, 0.561, 0.157],
        [0.862, 0.256, 0.107],
        [0.479, 0.015, 0.010],
    ],
    CORROSION: [
        [0.145, 0.388, 0.922],
        [0.231, 0.509, 0.965],
        [0.133, 0.773, 0.369],
        [0.518, 0.800, 0.086],
        [0.917, 0.788, 0.031],
        [0.961, 0.619, 0.043],
        [0.937, 0.270, 0.270],
        [0.596, 0.105, 0.105],
    ],
    PH: [
        [0.900, 0.200, 0.200],
        [0.930, 0.450, 0.250],
        [0.960, 0.750, 0.300],
        [0.980, 0.930, 0.350],
        [0.650, 0.950, 0.400],
        [0.300, 0.900, 0.600],
        [0.250, 0.700, 0.900],
        [0.200, 0.400, 0.950],
        [0.400, 0.250, 0.800],
    ],
};

function hexToRgb(hex) {
    const m = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
    return m ? [parseInt(m[1],16)/255, parseInt(m[2],16)/255, parseInt(m[3],16)/255] : [0,0,0];
}

function lerp(a, b, t) { return a + (b - a) * t; }

function lerpColor(c1, c2, t) {
    return [lerp(c1[0], c2[0], t), lerp(c1[1], c2[1], t), lerp(c1[2], c2[2], t)];
}

function sampleColorMap(value, min, max, mapName = 'CORROSION') {
    const map = COLOR_MAPS[mapName] || COLOR_MAPS.CORROSION;
    const clamped = Math.max(0, Math.min(1, (value - min) / (max - min)));
    const scaled = clamped * (map.length - 1);
    const idx = Math.floor(scaled);
    const frac = scaled - idx;
    if (idx >= map.length - 1) return map[map.length - 1];
    return lerpColor(map[idx], map[idx + 1], frac);
}

function rgbToCss(rgb, alpha = 1) {
    return `rgba(${(rgb[0]*255)|0},${(rgb[1]*255)|0},${(rgb[2]*255)|0},${alpha})`;
}

function rgbToHex(rgb) {
    const r = Math.max(0, Math.min(255, (rgb[0] * 255) | 0));
    const g = Math.max(0, Math.min(255, (rgb[1] * 255) | 0));
    const b = Math.max(0, Math.min(255, (rgb[2] * 255) | 0));
    return '#' + ((1<<24) + (r<<16) + (g<<8) + b).toString(16).slice(1);
}

function renderColorBar(canvasId, mapName, min, max, labelMin, labelMax) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const w = canvas.width, h = canvas.height;
    for (let x = 0; x < w; x++) {
        const t = x / (w - 1);
        const v = min + t * (max - min);
        const rgb = sampleColorMap(v, min, max, mapName);
        ctx.fillStyle = rgbToCss(rgb);
        ctx.fillRect(x, 0, 1, h);
    }
    ctx.fillStyle = '#fff';
    ctx.font = '10px monospace';
    ctx.textBaseline = 'middle';
    ctx.fillText(labelMin || min.toFixed(0), 4, h / 2);
    ctx.textAlign = 'right';
    ctx.fillText(labelMax || max.toFixed(0), w - 4, h / 2);
    ctx.textAlign = 'center';
    ctx.fillText(((min + max) / 2).toFixed(0), w / 2, h / 2);
}

function corrosionRiskLevel(depthUm) {
    if (depthUm < 20) return 'LOW';
    if (depthUm < 60) return 'MEDIUM';
    if (depthUm < 150) return 'HIGH';
    return 'CRITICAL';
}

function phRiskLevel(ph) {
    if (ph < 5.5 || ph > 8.5) return 'HIGH';
    if (ph < 6.0 || ph > 8.0) return 'MEDIUM';
    return 'LOW';
}

function caRiskLevel(ppm) {
    if (ppm > 200) return 'HIGH';
    if (ppm > 150) return 'MEDIUM';
    return 'LOW';
}

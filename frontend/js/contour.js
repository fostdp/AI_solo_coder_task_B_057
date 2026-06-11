function bilinearInterpolate(grid, size, x, y) {
    const x0 = Math.floor(x);
    const y0 = Math.floor(y);
    const x1 = Math.min(x0 + 1, size - 1);
    const y1 = Math.min(y0 + 1, size - 1);
    const fx = x - x0;
    const fy = y - y0;
    const v00 = grid[y0]?.[x0] ?? 0;
    const v10 = grid[y0]?.[x1] ?? 0;
    const v01 = grid[y1]?.[x0] ?? 0;
    const v11 = grid[y1]?.[x1] ?? 0;
    const a = v00 * (1 - fx) + v10 * fx;
    const b = v01 * (1 - fx) + v11 * fx;
    return a * (1 - fy) + b * fy;
}

function scatterToGrid(points, size = 50, defaultValue = null) {
    const grid = Array.from({ length: size }, () => new Array(size).fill(defaultValue));
    const counts = Array.from({ length: size }, () => new Array(size).fill(0));
    for (const p of points) {
        const xi = Math.max(0, Math.min(size - 1, Math.floor(p.x)));
        const yi = Math.max(0, Math.min(size - 1, Math.floor(p.y)));
        if (grid[yi][xi] === null) grid[yi][xi] = 0;
        grid[yi][xi] += p.value;
        counts[yi][xi] += 1;
    }
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            if (counts[y][x] > 0) {
                grid[y][x] /= counts[y][x];
            }
        }
    }
    fillMissingGrid(grid, size, defaultValue);
    return grid;
}

function fillMissingGrid(grid, size, sentinel) {
    let hasMissing = true;
    let iterations = 0;
    while (hasMissing && iterations < 50) {
        hasMissing = false;
        iterations++;
        const newGrid = grid.map(r => r.slice());
        for (let y = 0; y < size; y++) {
            for (let x = 0; x < size; x++) {
                if (grid[y][x] !== sentinel && grid[y][x] !== null) continue;
                let sum = 0, n = 0;
                for (let dy = -1; dy <= 1; dy++) {
                    for (let dx = -1; dx <= 1; dx++) {
                        const ny = y + dy, nx = x + dx;
                        if (nx < 0 || ny < 0 || nx >= size || ny >= size) continue;
                        const v = grid[ny][nx];
                        if (v !== null && v !== sentinel) {
                            const w = (dx === 0 && dy === 0) ? 0 : (Math.abs(dx) + Math.abs(dy) === 2 ? 0.5 : 1);
                            sum += v * w;
                            n += w;
                        }
                    }
                }
                if (n > 0) {
                    newGrid[y][x] = sum / n;
                } else {
                    hasMissing = true;
                }
            }
        }
        for (let y = 0; y < size; y++)
            for (let x = 0; x < size; x++)
                grid[y][x] = newGrid[y][x];
    }
    let fallback = 0;
    for (let y = 0; y < size; y++)
        for (let x = 0; x < size; x++)
            if (grid[y][x] !== null && !isNaN(grid[y][x]))
                fallback = grid[y][x];
    for (let y = 0; y < size; y++)
        for (let x = 0; x < size; x++)
            if (grid[y][x] === null || isNaN(grid[y][x]))
                grid[y][x] = fallback;
}

function marchingSquaresSegment(grid, size, cellX, cellY, threshold) {
    const x0 = cellX, y0 = cellY;
    const v = [
        grid[y0]?.[x0] ?? 0,
        grid[y0]?.[x0 + 1] ?? 0,
        grid[y0 + 1]?.[x0 + 1] ?? 0,
        grid[y0 + 1]?.[x0] ?? 0,
    ];
    let idx = 0;
    if (v[0] > threshold) idx |= 1;
    if (v[1] > threshold) idx |= 2;
    if (v[2] > threshold) idx |= 4;
    if (v[3] > threshold) idx |= 8;
    if (idx === 0 || idx === 15) return null;

    function interp(a, b, va, vb) {
        if (Math.abs(vb - va) < 1e-9) return (a + b) / 2;
        const t = (threshold - va) / (vb - va);
        return a + (b - a) * t;
    }

    const p = {
        top: [interp(x0, x0 + 1, v[0], v[1]), y0],
        right: [x0 + 1, interp(y0, y0 + 1, v[1], v[2])],
        bottom: [interp(x0, x0 + 1, v[3], v[2]), y0 + 1],
        left: [x0, interp(y0, y0 + 1, v[0], v[3])],
    };

    const segMap = {
        1:  [p.left, p.top],
        2:  [p.top, p.right],
        3:  [p.left, p.right],
        4:  [p.right, p.bottom],
        5:  [[p.left, p.top], [p.right, p.bottom]],
        6:  [p.top, p.bottom],
        7:  [p.left, p.bottom],
        8:  [p.left, p.bottom],
        9:  [p.top, p.bottom],
        10: [[p.top, p.left], [p.bottom, p.right]],
        11: [p.right, p.bottom],
        12: [p.left, p.right],
        13: [p.top, p.right],
        14: [p.left, p.top],
    };

    const segs = segMap[idx];
    if (!segs) return null;
    if (typeof segs[0][0] === 'number') return [segs];
    return segs;
}

function generateContours(grid, size, numLevels = 10) {
    let min = Infinity, max = -Infinity;
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            const v = grid[y][x];
            if (v < min) min = v;
            if (v > max) max = v;
        }
    }
    const levels = [];
    for (let i = 1; i <= numLevels; i++) {
        levels.push(min + (max - min) * (i / (numLevels + 1)));
    }

    const contours = [];
    for (const lvl of levels) {
        const segments = [];
        for (let y = 0; y < size - 1; y++) {
            for (let x = 0; x < size - 1; x++) {
                const segs = marchingSquaresSegment(grid, size, x, y, lvl);
                if (segs) segments.push(...segs);
            }
        }
        contours.push({ level: lvl, segments: chainSegments(segments) });
    }
    return { contours, min, max };
}

function chainSegments(segments) {
    if (segments.length < 2) return segments;
    const used = new Array(segments.length).fill(false);
    const chains = [];
    let remaining = segments.length;

    while (remaining > 0) {
        let startIdx = used.findIndex(u => !u);
        if (startIdx === -1) break;
        used[startIdx] = true;
        remaining--;
        const chain = [segments[startIdx][0], segments[startIdx][1]];
        let extended = true;
        while (extended && remaining > 0) {
            extended = false;
            const tail = chain[chain.length - 1];
            const head = chain[0];
            for (let i = 0; i < segments.length; i++) {
                if (used[i]) continue;
                const s = segments[i];
                const d_tail_s0 = dist2(tail, s[0]);
                const d_tail_s1 = dist2(tail, s[1]);
                const d_head_s0 = dist2(head, s[0]);
                const d_head_s1 = dist2(head, s[1]);
                const eps = 1e-4;
                if (d_tail_s0 < eps) { chain.push(s[1]); used[i] = true; remaining--; extended = true; break; }
                if (d_tail_s1 < eps) { chain.push(s[0]); used[i] = true; remaining--; extended = true; break; }
                if (d_head_s1 < eps) { chain.unshift(s[0]); used[i] = true; remaining--; extended = true; break; }
                if (d_head_s0 < eps) { chain.unshift(s[1]); used[i] = true; remaining--; extended = true; break; }
            }
        }
        chains.push(chain);
    }
    return chains;
}

function dist2(a, b) {
    const dx = a[0] - b[0], dy = a[1] - b[1];
    return dx * dx + dy * dy;
}

function drawContourMap(canvasId, points, options = {}) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const W = canvas.width, H = canvas.height;
    const size = options.gridSize || 50;
    const colorMap = options.colorMap || 'PH';
    const showLabels = options.showLabels !== false;
    const numLevels = options.numLevels || 10;

    ctx.fillStyle = '#0c1014';
    ctx.fillRect(0, 0, W, H);

    const grid = scatterToGrid(points, size);
    const { contours, min, max } = generateContours(grid, size, numLevels);

    const cellW = W / size, cellH = H / size;
    const imgData = ctx.createImageData(W, H);
    for (let py = 0; py < H; py++) {
        for (let px = 0; px < W; px++) {
            const gx = px / cellW - 0.5;
            const gy = py / cellH - 0.5;
            const val = bilinearInterpolate(grid, size, gx, gy);
            const rgb = sampleColorMap(val, min, max, colorMap);
            const idx = (py * W + px) * 4;
            imgData.data[idx] = (rgb[0] * 255) | 0;
            imgData.data[idx + 1] = (rgb[1] * 255) | 0;
            imgData.data[idx + 2] = (rgb[2] * 255) | 0;
            imgData.data[idx + 3] = 230;
        }
    }
    ctx.putImageData(imgData, 0, 0);

    ctx.strokeStyle = 'rgba(255,255,255,0.07)';
    ctx.lineWidth = 1;
    for (let i = 0; i <= size; i += 5) {
        ctx.beginPath();
        ctx.moveTo(i * cellW, 0); ctx.lineTo(i * cellW, H);
        ctx.moveTo(0, i * cellH); ctx.lineTo(W, i * cellH);
        ctx.stroke();
    }

    for (const { level, segments: chains } of contours) {
        const t = (level - min) / (max - min);
        const darkness = t > 0.5 ? 0.9 : 0.2;
        ctx.strokeStyle = `rgba(255,255,255,${0.4 + darkness * 0.4})`;
        ctx.lineWidth = t > 0.85 || t < 0.15 ? 1.8 : 1;
        for (const chain of chains) {
            if (chain.length < 2) continue;
            ctx.beginPath();
            ctx.moveTo(chain[0][0] * cellW, chain[0][1] * cellH);
            for (let i = 1; i < chain.length; i++) {
                ctx.lineTo(chain[i][0] * cellW, chain[i][1] * cellH);
            }
            ctx.stroke();
        }

        if (showLabels && chains.length > 0 && chains[0].length > 4) {
            const mid = chains[0][Math.floor(chains[0].length / 2)];
            const tx = mid[0] * cellW + 4;
            const ty = mid[1] * cellH - 4;
            if (tx > 10 && tx < W - 50 && ty > 10 && ty < H - 10) {
                ctx.fillStyle = 'rgba(0,0,0,0.6)';
                const label = options.valueFormatter ? options.valueFormatter(level) : level.toFixed(1);
                ctx.font = '10px monospace';
                const tw = ctx.measureText(label).width;
                ctx.fillRect(tx - 2, ty - 9, tw + 6, 13);
                ctx.fillStyle = '#fff';
                ctx.fillText(label, tx + 1, ty);
            }
        }
    }

    ctx.strokeStyle = 'rgba(212,168,85,0.5)';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([4, 3]);
    if (options.highlightY !== undefined) {
        const yy = options.highlightY * cellH;
        ctx.beginPath(); ctx.moveTo(0, yy); ctx.lineTo(W, yy); ctx.stroke();
    }
    ctx.setLineDash([]);

    if (options.relicPoints) {
        for (const r of options.relicPoints) {
            const rx = r.x * cellW, ry = r.y * cellH;
            ctx.beginPath();
            ctx.arc(rx, ry, 3, 0, Math.PI * 2);
            ctx.fillStyle = 'rgba(212,168,85,0.8)';
            ctx.fill();
            ctx.strokeStyle = '#fff';
            ctx.lineWidth = 0.5;
            ctx.stroke();
        }
    }

    return { min, max, grid };
}

function extractProfile(grid, yIndex, size) {
    const profile = [];
    for (let x = 0; x < size; x++) {
        profile.push(grid[yIndex]?.[x] ?? 0);
    }
    return profile;
}

function drawGridHeatmap(canvasId, points, colorMap = 'PH', valueFormatter = null) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const W = canvas.width, H = canvas.height;
    const size = 50;

    ctx.fillStyle = '#0c1014';
    ctx.fillRect(0, 0, W, H);

    const grid = scatterToGrid(points, size);
    let min = Infinity, max = -Infinity;
    for (let y = 0; y < size; y++)
        for (let x = 0; x < size; x++) {
            if (grid[y][x] < min) min = grid[y][x];
            if (grid[y][x] > max) max = grid[y][x];
        }

    const cellW = W / size, cellH = H / size;
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            const v = grid[y][x];
            const rgb = sampleColorMap(v, min, max, colorMap);
            ctx.fillStyle = rgbToCss(rgb, 0.9);
            ctx.fillRect(x * cellW, y * cellH, cellW + 0.5, cellH + 0.5);
        }
    }

    ctx.strokeStyle = 'rgba(255,255,255,0.05)';
    ctx.lineWidth = 0.5;
    for (let i = 0; i <= size; i += 5) {
        ctx.beginPath();
        ctx.moveTo(i * cellW, 0); ctx.lineTo(i * cellW, H);
        ctx.moveTo(0, i * cellH); ctx.lineTo(W, i * cellH);
        ctx.stroke();
    }
    return { min, max };
}

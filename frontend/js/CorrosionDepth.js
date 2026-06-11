const CorrosionDepth = (function() {

    const DEPTH_THRESHOLDS = {
        LOW: 20,
        MEDIUM: 50,
        HIGH: 100,
        CRITICAL: 200
    };

    const RISK_LEVELS = {
        '低': { color: '#34c759', order: 1 },
        '中': { color: '#ffcc00', order: 2 },
        '高': { color: '#ff9500', order: 3 },
        '极高': { color: '#ff3b30', order: 4 }
    };

    function analyzeDepths(points) {
        if (!points || points.length === 0) {
            return null;
        }

        let maxDepth = 0;
        let minDepth = Infinity;
        let sumDepth = 0;
        let sumCollagen = 0;
        let maxDepthPt = null;
        let depthHistogram = new Array(10).fill(0);

        for (let i = 0; i < points.length; i++) {
            const p = points[i];
            const d = p.corrosion_depth || 0;
            const c = p.collagen_deg || 0;

            if (d > maxDepth) {
                maxDepth = d;
                maxDepthPt = p;
            }
            if (d < minDepth) minDepth = d;
            sumDepth += d;
            sumCollagen += c;

            const bucket = Math.min(9, Math.floor(d / 50));
            depthHistogram[bucket]++;
        }

        if (maxDepth < 100) maxDepth = 300;

        const avgDepth = sumDepth / points.length;
        const avgCollagen = sumCollagen / points.length;

        return {
            count: points.length,
            maxDepth,
            minDepth,
            avgDepth,
            avgCollagen,
            maxDepthPoint: maxDepthPt,
            histogram: depthHistogram,
            histogramBuckets: Array.from({length: 10}, (_, i) => i * 50)
        };
    }

    function classifyDepth(depth) {
        if (depth < DEPTH_THRESHOLDS.LOW) return 'low';
        if (depth < DEPTH_THRESHOLDS.MEDIUM) return 'medium';
        if (depth < DEPTH_THRESHOLDS.HIGH) return 'high';
        if (depth < DEPTH_THRESHOLDS.CRITICAL) return 'severe';
        return 'critical';
    }

    function depthToColor(depth, maxDepth, colorMode) {
        switch (colorMode) {
            case 'collagen':
                return sampleColorMap(depth, 0, Math.max(maxDepth, 50), 'PLASMA');
            case 'height':
                return sampleColorMap(depth, -5, 5, 'VIRIDIS');
            case 'depth':
            default:
                return sampleColorMap(depth, 0, maxDepth, 'CORROSION');
        }
    }

    function computePointColors(points, colorMode) {
        if (!points || points.length === 0) {
            return { colors: new Float32Array(), maxDepth: 0, maxCollagen: 0, analysis: null };
        }

        const analysis = analyzeDepths(points);
        const colors = new Float32Array(points.length * 3);

        let maxZ = -Infinity, minZ = Infinity;
        for (let i = 0; i < points.length; i++) {
            if (points[i].z > maxZ) maxZ = points[i].z;
            if (points[i].z < minZ) minZ = points[i].z;
        }

        for (let i = 0; i < points.length; i++) {
            const p = points[i];
            let value;
            let colorMax;

            switch (colorMode) {
                case 'collagen':
                    value = p.collagen_deg || 0;
                    colorMax = Math.max(analysis.maxDepth, 50);
                    break;
                case 'height':
                    value = p.z;
                    colorMax = maxZ;
                    break;
                case 'depth':
                default:
                    value = p.corrosion_depth || 0;
                    colorMax = analysis.maxDepth;
                    break;
            }

            const rgb = depthToColor(value, colorMax, colorMode);
            colors[i * 3] = rgb[0];
            colors[i * 3 + 1] = rgb[1];
            colors[i * 3 + 2] = rgb[2];
        }

        return { colors, maxDepth: analysis.maxDepth, maxCollagen: analysis.avgCollagen, analysis, minZ, maxZ };
    }

    function assessRisk(ph, caPpm, degPct, depthUm) {
        let score = 0;
        if (ph < 5.5) score += 2;
        else if (ph < 6.0) score += 1;

        if (caPpm > 200) score += 2;
        else if (caPpm > 100) score += 1;

        if (degPct > 30) score += 2;
        else if (degPct > 15) score += 1;

        if (depthUm > 50) score += 2;
        else if (depthUm > 20) score += 1;

        if (score <= 2) return '低';
        if (score <= 4) return '中';
        if (score <= 6) return '高';
        return '极高';
    }

    function riskColor(riskLevel) {
        return RISK_LEVELS[riskLevel] ? RISK_LEVELS[riskLevel].color : '#888';
    }

    function formatDepth(um) {
        if (um >= 1000) return (um / 1000).toFixed(2) + ' mm';
        if (um >= 1) return um.toFixed(1) + ' μm';
        if (um >= 0.001) return (um * 1000).toFixed(1) + ' nm';
        return '0';
    }

    return {
        DEPTH_THRESHOLDS,
        RISK_LEVELS,
        analyzeDepths,
        classifyDepth,
        depthToColor,
        computePointColors,
        assessRisk,
        riskColor,
        formatDepth
    };
})();

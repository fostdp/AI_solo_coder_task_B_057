const BoneCloud = (function() {

    let renderer = null;
    let scene = null;
    let camera = null;
    let controls = null;
    let cloudGroup = null;
    let infoSprites = null;
    let container = null;
    let animationId = null;
    let pointLight = null;

    let dirty = true;
    let lastInteractionTime = 0;
    const IDLE_THRESHOLD_MS = 5000;
    const STATIC_FRAME_INTERVAL = 2000;
    let lastStaticRender = 0;
    let currentPoints = null;
    let currentColorMode = 'depth';

    function init(containerId) {
        container = document.getElementById(containerId);
        if (!container) return null;

        if (renderer) {
            dispose();
            container.innerHTML = '';
        }

        const width = container.clientWidth;
        const height = container.clientHeight;

        scene = new THREE.Scene();
        scene.background = new THREE.Color(0x0c1014);
        scene.fog = new THREE.Fog(0x0c1014, 30, 80);

        camera = new THREE.PerspectiveCamera(50, width / height, 0.1, 1000);
        camera.position.set(15, 12, 18);

        renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
        renderer.setSize(width, height);
        renderer.setPixelRatio(window.devicePixelRatio || 1);
        renderer.shadowMap.enabled = false;
        container.appendChild(renderer.domElement);

        controls = new THREE.OrbitControls(camera, renderer.domElement);
        controls.enableDamping = true;
        controls.dampingFactor = 0.08;
        controls.minDistance = 5;
        controls.maxDistance = 60;
        controls.autoRotate = false;
        controls.autoRotateSpeed = 0.8;
        controls.target.set(0, 0, 0);

        const ambientLight = new THREE.AmbientLight(0xffffff, 0.55);
        scene.add(ambientLight);

        const dirLight = new THREE.DirectionalLight(0xfff0d5, 0.8);
        dirLight.position.set(10, 20, 15);
        scene.add(dirLight);

        const backLight = new THREE.DirectionalLight(0x88aaff, 0.35);
        backLight.position.set(-10, 10, -15);
        scene.add(backLight);

        pointLight = new THREE.PointLight(0xd4a855, 0.8, 50);
        pointLight.position.set(0, 10, 0);
        scene.add(pointLight);

        addAxesAndGrid();

        cloudGroup = new THREE.Group();
        scene.add(cloudGroup);
        infoSprites = new THREE.Group();
        scene.add(infoSprites);

        controls.addEventListener('change', () => {
            dirty = true;
            lastInteractionTime = performance.now();
        });

        window.addEventListener('resize', onResize);

        animate(0);
        return getInstance();
    }

    function addAxesAndGrid() {
        const axes = new THREE.AxesHelper(8);
        axes.position.set(-12, -5, -12);
        scene.add(axes);

        const gridHelper = new THREE.GridHelper(30, 30, 0x3a4656, 0x1e2a38);
        gridHelper.position.y = -6;
        scene.add(gridHelper);

        const planeGeo = new THREE.PlaneGeometry(30, 30);
        const planeMat = new THREE.MeshPhongMaterial({
            color: 0x1a222d,
            transparent: true,
            opacity: 0.5,
            side: THREE.DoubleSide,
        });
        const plane = new THREE.Mesh(planeGeo, planeMat);
        plane.rotation.x = -Math.PI / 2;
        plane.position.y = -6.01;
        scene.add(plane);

        makeAxisLabel('X', -2, -5, -12, 0xff6666);
        makeAxisLabel('Y', -12, 3.5, -12, 0x66ff66);
        makeAxisLabel('Z', -12, -5, 1.5, 0x6688ff);
    }

    function makeAxisLabel(text, x, y, z, color) {
        const canvas = document.createElement('canvas');
        canvas.width = 64; canvas.height = 64;
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = 'rgba(0,0,0,0)';
        ctx.fillRect(0, 0, 64, 64);
        ctx.fillStyle = '#fff';
        ctx.font = 'bold 28px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.strokeStyle = '#' + color.toString(16).padStart(6, '0');
        ctx.lineWidth = 3;
        ctx.strokeText(text, 32, 32);
        ctx.fillText(text, 32, 32);
        const tex = new THREE.CanvasTexture(canvas);
        const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false });
        const sp = new THREE.Sprite(mat);
        sp.position.set(x, y, z);
        sp.scale.set(2, 2, 1);
        scene.add(sp);
    }

    function render(pointsArray, colorMode = 'depth') {
        if (!renderer) {
            init('three-container');
        }
        if (!pointsArray || pointsArray.length === 0) return null;

        currentPoints = pointsArray;
        currentColorMode = colorMode;

        clearGroup(cloudGroup);
        clearGroup(infoSprites, true);

        const { colors, maxDepth, analysis } = CorrosionDepth.computePointColors(pointsArray, colorMode);

        const positions = new Float32Array(pointsArray.length * 3);
        for (let i = 0; i < pointsArray.length; i++) {
            const p = pointsArray[i];
            positions[i * 3] = p.x;
            positions[i * 3 + 1] = p.z;
            positions[i * 3 + 2] = p.y;
        }

        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

        const material = new THREE.PointsMaterial({
            size: 0.12,
            vertexColors: true,
            sizeAttenuation: true,
            transparent: true,
            opacity: 0.92,
        });

        const points = new THREE.Points(geometry, material);
        cloudGroup.add(points);

        addCorrosionHull(pointsArray, maxDepth, colorMode);

        if (analysis && analysis.maxDepthPoint) {
            addMaxDepthMarker(analysis.maxDepthPoint);
        }

        const bbox = new THREE.Box3().setFromObject(points);
        const center = new THREE.Vector3();
        bbox.getCenter(center);
        controls.target.copy(center);
        const size = new THREE.Vector3();
        bbox.getSize(size);
        const maxDim = Math.max(size.x, size.y, size.z);
        camera.position.copy(center);
        camera.position.x += maxDim * 1.5;
        camera.position.y += maxDim * 0.9;
        camera.position.z += maxDim * 1.5;
        camera.lookAt(center);
        controls.update();

        dirty = true;

        return {
            avgDepth: analysis.avgDepth,
            maxDepth: analysis.maxDepth,
            maxDepthPt: analysis.maxDepthPoint,
            avgCollagen: analysis.avgCollagen,
            numPoints: pointsArray.length,
        };
    }

    function addCorrosionHull(points, maxDepth, colorMode) {
        const threshold = maxDepth * 0.7;
        const hullPoints = points.filter(p => p.corrosion_depth >= threshold);
        if (hullPoints.length < 4) return;

        const positions = new Float32Array(hullPoints.length * 3);
        const colors = new Float32Array(hullPoints.length * 3);

        for (let i = 0; i < hullPoints.length; i++) {
            const p = hullPoints[i];
            positions[i * 3] = p.x;
            positions[i * 3 + 1] = p.z + 0.02;
            positions[i * 3 + 2] = p.y;
            const rgb = CorrosionDepth.depthToColor(p.corrosion_depth, maxDepth, colorMode);
            colors[i * 3] = rgb[0];
            colors[i * 3 + 1] = rgb[1];
            colors[i * 3 + 2] = rgb[2];
        }

        const geo = new THREE.BufferGeometry();
        geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geo.setAttribute('color', new THREE.BufferAttribute(colors, 3));

        const mat = new THREE.PointsMaterial({
            size: 0.22,
            vertexColors: true,
            transparent: true,
            opacity: 0.7,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
        });

        const hull = new THREE.Points(geo, mat);
        cloudGroup.add(hull);
    }

    function addMaxDepthMarker(point) {
        const canvas = document.createElement('canvas');
        canvas.width = 256; canvas.height = 64;
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = 'rgba(0,0,0,0)';
        ctx.fillRect(0, 0, 256, 64);
        ctx.fillStyle = '#ff3b30';
        ctx.strokeStyle = '#fff';
        ctx.lineWidth = 2;
        ctx.font = 'bold 18px sans-serif';
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        const text = '最大腐蚀 ' + point.corrosion_depth.toFixed(1) + 'μm';
        ctx.strokeText(text, 128, 32);
        ctx.fillText(text, 128, 32);
        const tex = new THREE.CanvasTexture(canvas);
        const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false });
        const sp = new THREE.Sprite(mat);
        sp.position.set(point.x, point.z + 1.5, point.y);
        sp.scale.set(5, 1.2, 1);
        infoSprites.add(sp);

        const ringGeo = new THREE.RingGeometry(0.3, 0.5, 32);
        const ringMat = new THREE.MeshBasicMaterial({ color: 0xff3b30, side: THREE.DoubleSide, transparent: true, opacity: 0.8 });
        const ring = new THREE.Mesh(ringGeo, ringMat);
        ring.rotation.x = -Math.PI / 2;
        ring.position.set(point.x, point.z + 0.05, point.y);
        infoSprites.add(ring);
    }

    function clearGroup(group, disposeMaps = false) {
        while (group.children.length > 0) {
            const child = group.children[0];
            group.remove(child);
            if (child.geometry) child.geometry.dispose();
            if (child.material) {
                if (Array.isArray(child.material)) {
                    child.material.forEach(m => {
                        if (disposeMaps && m.map) m.map.dispose();
                        m.dispose();
                    });
                } else {
                    if (disposeMaps && child.material.map) child.material.map.dispose();
                    child.material.dispose();
                }
            }
        }
    }

    function animate(time) {
        animationId = requestAnimationFrame(animate);

        const timeSinceInteraction = time - lastInteractionTime;
        const isInteracting = timeSinceInteraction < 200;
        const pointLightMoving = true;

        if (isInteracting || dirty || pointLightMoving) {
            controls.update();
            const t = time * 0.001;
            pointLight.position.x = Math.sin(t * 0.3) * 8;
            pointLight.position.z = Math.cos(t * 0.3) * 8;
            renderer.render(scene, camera);
            dirty = false;
            lastStaticRender = time;
        } else if (time - lastStaticRender > STATIC_FRAME_INTERVAL) {
            controls.update();
            const t = time * 0.001;
            pointLight.position.x = Math.sin(t * 0.3) * 8;
            pointLight.position.z = Math.cos(t * 0.3) * 8;
            renderer.render(scene, camera);
            lastStaticRender = time;
        }
    }

    function onResize() {
        if (!container || !camera || !renderer) return;
        const w = container.clientWidth;
        const h = container.clientHeight;
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
        renderer.setSize(w, h);
        dirty = true;
    }

    function resetView() {
        if (camera && controls) {
            camera.position.set(15, 12, 18);
            controls.target.set(0, 0, 0);
            controls.update();
            dirty = true;
        }
    }

    function markDirty() {
        dirty = true;
    }

    function getAnalysis() {
        if (!currentPoints) return null;
        return CorrosionDepth.analyzeDepths(currentPoints);
    }

    function getInstance() {
        return {
            render,
            resetView,
            markDirty,
            getAnalysis,
            getCurrentPoints: () => currentPoints,
            getColorMode: () => currentColorMode,
        };
    }

    function dispose() {
        if (animationId) cancelAnimationFrame(animationId);
        window.removeEventListener('resize', onResize);
        if (controls) controls.dispose();
        if (renderer) renderer.dispose();
        renderer = null;
        scene = null;
        camera = null;
        controls = null;
        cloudGroup = null;
        infoSprites = null;
        pointLight = null;
        currentPoints = null;
    }

    return {
        init,
        render,
        resetView,
        dispose,
        markDirty,
        getAnalysis,
    };
})();

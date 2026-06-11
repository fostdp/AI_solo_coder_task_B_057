let pointCloudRenderer = null;

function initThreeScene(containerId) {
    const container = document.getElementById(containerId);
    if (!container) return null;

    if (pointCloudRenderer) {
        try {
            pointCloudRenderer.renderer.dispose();
            pointCloudRenderer.controls.dispose();
        } catch (e) {}
        container.innerHTML = '';
    }

    const width = container.clientWidth;
    const height = container.clientHeight;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0c1014);
    scene.fog = new THREE.Fog(0x0c1014, 30, 80);

    const camera = new THREE.PerspectiveCamera(50, width / height, 0.1, 1000);
    camera.position.set(15, 12, 18);

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
    renderer.setSize(width, height);
    renderer.setPixelRatio(window.devicePixelRatio || 1);
    renderer.shadowMap.enabled = false;
    container.appendChild(renderer.domElement);

    const controls = new THREE.OrbitControls(camera, renderer.domElement);
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

    const pointLight = new THREE.PointLight(0xd4a855, 0.8, 50);
    pointLight.position.set(0, 10, 0);
    scene.add(pointLight);

    addAxesAndGrid(scene);

    let animationId = null;
    const cloudGroup = new THREE.Group();
    scene.add(cloudGroup);
    const infoSprites = new THREE.Group();
    scene.add(infoSprites);

    let dirty = true;
    let lastInteractionTime = 0;
    const IDLE_THRESHOLD_MS = 5000;
    const STATIC_FRAME_INTERVAL = 2000;
    let lastStaticRender = 0;

    controls.addEventListener('change', () => {
        dirty = true;
        lastInteractionTime = performance.now();
    });

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
    animate(0);

    function markDirty() {
        dirty = true;
    }

    function onResize() {
        const w = container.clientWidth;
        const h = container.clientHeight;
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
        renderer.setSize(w, h);
        dirty = true;
    }
    window.addEventListener('resize', onResize);

    pointCloudRenderer = {
        scene, camera, renderer, controls,
        cloudGroup, infoSprites, container,
        animationId, onResize, markDirty,
        dispose() {
            cancelAnimationFrame(animationId);
            window.removeEventListener('resize', onResize);
            controls.dispose();
            renderer.dispose();
            container.innerHTML = '';
        }
    };
    return pointCloudRenderer;
}

function addAxesAndGrid(scene) {
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

    makeAxisLabel(scene, 'X', -2, -5, -12, 0xff6666);
    makeAxisLabel(scene, 'Y', -12, 3.5, -12, 0x66ff66);
    makeAxisLabel(scene, 'Z', -12, -5, 1.5, 0x6688ff);
}

function makeAxisLabel(scene, text, x, y, z, color) {
    const canvas = document.createElement('canvas');
    canvas.width = 64; canvas.height = 64;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = 'rgba(0,0,0,0)';
    ctx.fillRect(0, 0, 64, 64);
    ctx.fillStyle = '#fff';
    ctx.font = 'bold 28px sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.strokeStyle = color;
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

function renderPointCloud(pointsArray, colorMode = 'depth') {
    if (!pointCloudRenderer) {
        initThreeScene('three-container');
    }
    const { cloudGroup, infoSprites } = pointCloudRenderer;

    while (cloudGroup.children.length > 0) {
        const c = cloudGroup.children[0];
        cloudGroup.remove(c);
        if (c.geometry) c.geometry.dispose();
        if (c.material) {
            if (Array.isArray(c.material)) c.material.forEach(m => m.dispose());
            else c.material.dispose();
        }
    }
    while (infoSprites.children.length > 0) {
        const c = infoSprites.children[0];
        infoSprites.remove(c);
        if (c.material?.map) c.material.map.dispose();
        if (c.material) c.material.dispose();
    }

    if (!pointsArray || pointsArray.length === 0) return;

    const positions = new Float32Array(pointsArray.length * 3);
    const colors = new Float32Array(pointsArray.length * 3);
    let maxDepth = 0, maxCollagen = 0, maxZ = -Infinity, minZ = Infinity;

    for (let i = 0; i < pointsArray.length; i++) {
        const p = pointsArray[i];
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.z;
        positions[i * 3 + 2] = p.y;
        if (p.corrosion_depth > maxDepth) maxDepth = p.corrosion_depth;
        if (p.collagen_deg > maxCollagen) maxCollagen = p.collagen_deg;
        if (p.z > maxZ) maxZ = p.z;
        if (p.z < minZ) minZ = p.z;
    }
    if (maxDepth < 100) maxDepth = 300;

    let sumDepth = 0, sumColl = 0, maxDepthPt = null;
    for (let i = 0; i < pointsArray.length; i++) {
        const p = pointsArray[i];
        let rgb;
        switch (colorMode) {
            case 'collagen':
                rgb = sampleColorMap(p.collagen_deg, 0, Math.max(maxCollagen, 50), 'PLASMA');
                break;
            case 'height':
                rgb = sampleColorMap(p.z, minZ, maxZ, 'VIRIDIS');
                break;
            case 'depth':
            default:
                rgb = sampleColorMap(p.corrosion_depth, 0, maxDepth, 'CORROSION');
                break;
        }
        colors[i * 3] = rgb[0];
        colors[i * 3 + 1] = rgb[1];
        colors[i * 3 + 2] = rgb[2];

        sumDepth += p.corrosion_depth;
        sumColl += p.collagen_deg;
        if (!maxDepthPt || p.corrosion_depth > maxDepthPt.corrosion_depth) {
            maxDepthPt = p;
        }
    }

    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));

    const mat = new THREE.PointsMaterial({
        size: 0.14,
        vertexColors: true,
        transparent: true,
        opacity: 0.95,
        sizeAttenuation: true,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
    });

    const pointsObj = new THREE.Points(geom, mat);
    cloudGroup.add(pointsObj);

    addCorrosionHull(cloudGroup, pointsArray, maxDepth, colorMode);
    addMaxDepthMarker(infoSprites, maxDepthPt);

    const bbox = new THREE.Box3().setFromObject(pointsObj);
    const center = new THREE.Vector3();
    bbox.getCenter(center);
    pointCloudRenderer.controls.target.copy(center);
    const size = new THREE.Vector3();
    bbox.getSize(size);
    const maxDim = Math.max(size.x, size.y, size.z);
    pointCloudRenderer.camera.position.copy(center);
    pointCloudRenderer.camera.position.x += maxDim * 1.5;
    pointCloudRenderer.camera.position.y += maxDim * 0.9;
    pointCloudRenderer.camera.position.z += maxDim * 1.5;
    pointCloudRenderer.camera.lookAt(center);
    pointCloudRenderer.controls.update();

    return {
        avgDepth: sumDepth / pointsArray.length,
        maxDepth,
        maxDepthPt,
        avgCollagen: sumColl / pointsArray.length,
        numPoints: pointsArray.length,
    };
}

function addCorrosionHull(group, points, maxDepth, colorMode) {
    const hullPoints = points.filter(p => p.corrosion_depth > maxDepth * 0.75);
    if (hullPoints.length < 10) return;

    const positions = new Float32Array(hullPoints.length * 3);
    const colors = new Float32Array(hullPoints.length * 3);
    for (let i = 0; i < hullPoints.length; i++) {
        const p = hullPoints[i];
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.z;
        positions[i * 3 + 2] = p.y;
        const rgb = [1, 0.1 + (1 - p.corrosion_depth / maxDepth) * 0.4, 0.1];
        colors[i * 3] = rgb[0]; colors[i * 3 + 1] = rgb[1]; colors[i * 3 + 2] = rgb[2];
    }
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    const mat = new THREE.PointsMaterial({
        size: 0.22,
        vertexColors: true,
        transparent: true,
        opacity: 0.9,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
    });
    const hullPts = new THREE.Points(geom, mat);
    group.add(hullPts);
}

function addMaxDepthMarker(group, point) {
    if (!point) return;
    const canvas = document.createElement('canvas');
    canvas.width = 256; canvas.height = 128;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = 'rgba(244,90,90,0.92)';
    ctx.roundRect ? ctx.roundRect(4, 4, 248, 120, 12) : ctx.fillRect(4, 4, 248, 120);
    ctx.fill();
    ctx.strokeStyle = '#fff';
    ctx.lineWidth = 2;
    (ctx.roundRect ? ctx.roundRect(4, 4, 248, 120, 12) : null);
    ctx.stroke && ctx.stroke();
    ctx.fillStyle = '#fff';
    ctx.font = 'bold 20px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('⚠ MAX 腐蚀点', 128, 36);
    ctx.font = 'bold 30px monospace';
    ctx.fillStyle = '#ffdcdc';
    ctx.fillText(`${point.corrosion_depth.toFixed(1)} μm`, 128, 75);
    ctx.font = '13px sans-serif';
    ctx.fillStyle = '#ffeeee';
    ctx.fillText(`胶原降解: ${point.collagen_deg.toFixed(1)}%`, 128, 105);
    const tex = new THREE.CanvasTexture(canvas);
    tex.needsUpdate = true;
    const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: false });
    const sp = new THREE.Sprite(mat);
    sp.position.set(point.x, point.z + 3, point.y);
    sp.scale.set(5, 2.5, 1);
    sp.renderOrder = 999;
    group.add(sp);

    const lineGeo = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(point.x, point.z, point.y),
        new THREE.Vector3(point.x, point.z + 2.2, point.y),
    ]);
    const lineMat = new THREE.LineBasicMaterial({ color: 0xf45a5a, linewidth: 2, transparent: true, opacity: 0.8 });
    group.add(new THREE.Line(lineGeo, lineMat));
}

function resetThreeView() {
    if (!pointCloudRenderer) return;
    const { camera, controls, cloudGroup } = pointCloudRenderer;
    const bbox = new THREE.Box3().setFromObject(cloudGroup);
    const center = new THREE.Vector3();
    bbox.getCenter(center);
    const size = new THREE.Vector3();
    bbox.getSize(size);
    const maxDim = Math.max(size.x, size.y, size.z) || 15;
    controls.target.copy(center);
    camera.position.copy(center);
    camera.position.x += maxDim * 1.5;
    camera.position.y += maxDim * 0.9;
    camera.position.z += maxDim * 1.5;
    camera.lookAt(center);
    controls.update();
}

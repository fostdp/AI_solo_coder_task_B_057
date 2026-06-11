const API_CONFIG = {
    BASE_URL: window.location.origin.includes('localhost') || window.location.protocol === 'file:'
        ? 'http://127.0.0.1:8080'
        : '',
    REFRESH_INTERVAL_MS: 30000,
    AUTO_REFRESH: true,
};

const THRESHOLDS = {
    PH_LOW: 5.5,
    PH_HIGH: 8.5,
    CA_HIGH: 200,
    ORP_LOW: -200,
    ORP_HIGH: 400,
    TEMP_HIGH: 35,
    CORROSION_DEPTH_MAX: 300,
    COLLAGEN_DEG_MAX: 100,
};

const GRID = {
    MIN_X: 0,
    MAX_X: 50,
    MIN_Y: 0,
    MAX_Y: 50,
    CELL_SIZE: 1,
};

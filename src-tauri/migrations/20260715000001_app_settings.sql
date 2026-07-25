-- 应用全局设置表(key-value 结构,便于扩展)
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 初始化默认值(空字符串表示"使用系统默认")
INSERT OR IGNORE INTO app_settings (key, value) VALUES
    ('ollama_base_url', ''),
    ('ollama_registry', ''),
    ('https_proxy', ''),
    ('use_custom_models_dir', 'false'),
    ('download_max_retries', '3'),
    ('download_connect_timeout_secs', '30'),
    ('download_request_timeout_secs', '600');

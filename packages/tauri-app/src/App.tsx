import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface Connection {
  id: string;
  name: string;
  connection_type: string;
  mount_point: string | null;
  auto_mount: boolean;
  enabled: boolean;
  host?: string;
  username?: string;
  share?: string;       // SMB 共享名称
  remote_path?: string; // SMB 远程路径
}

interface Toast {
  message: string;
  type: 'success' | 'error' | 'info';
}

interface AppSettings {
  theme_mode: string;
  start_minimized: boolean;
  auto_start: boolean;
  log_level: string;
}

interface LogEntry {
  time: string;
  level: string;
  message: string;
}

function App() {
  const [connections, setConnections] = useState<Connection[]>([]);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showEditModal, setShowEditModal] = useState(false);
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  const [showAboutModal, setShowAboutModal] = useState(false);
  const [editingConnection, setEditingConnection] = useState<Connection | null>(null);
  const [loading, setLoading] = useState(true);
  const [toast, setToast] = useState<Toast | null>(null);
  const [mountingId, setMountingId] = useState<string | null>(null);
  const [mountedCount, setMountedCount] = useState(0);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [showLogs, setShowLogs] = useState(false);
  const logEndRef = useRef<HTMLDivElement>(null);

  // 加载设置并检测系统主题
  useEffect(() => {
    loadConnections();
    loadSettings();

    // 监听后端日志事件
    const unlisten = listen<{ level: string; message: string }>('log-event', (event) => {
      const now = new Date();
      const time = now.toLocaleTimeString('zh-CN', { hour12: false });
      setLogs(prev => [...prev.slice(-99), { time, level: event.payload.level, message: event.payload.message }]);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  // 加载设置
  const loadSettings = async () => {
    try {
      const settings = await invoke<AppSettings>('get_settings');
      applyTheme(settings.theme_mode);
    } catch (error) {
      console.error('Failed to load settings:', error);
      applyTheme('system');
    }
  };

  // 根据主题设置应用主题
  const applyTheme = (mode: string) => {
    let isDark: boolean;
    if (mode === 'dark') {
      isDark = true;
    } else if (mode === 'light') {
      isDark = false;
    } else {
      // system
      isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
    document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
  };

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [logs]);

  useEffect(() => {
    if (toast) {
      // 错误信息显示更长
      const duration = toast.type === 'error' ? 6000 : 3000;
      const timer = setTimeout(() => setToast(null), duration);
      // 同时添加日志
      const now = new Date();
      const time = now.toLocaleTimeString('zh-CN', { hour12: false });
      setLogs(prev => [...prev.slice(-99), { time, level: toast.type, message: toast.message }]);
      return () => clearTimeout(timer);
    }
  }, [toast]);

  useEffect(() => {
    setMountedCount(connections.filter(c => c.enabled).length);
  }, [connections]);

  const loadConnections = async () => {
    setLoading(true);
    try {
      // First sync existing SMB connections from system to config
      await invoke<Connection[]>('sync_existing_connections');
      // Then get the updated connection list
      const conns = await invoke<Connection[]>('get_connections');
      setConnections(conns);
    } catch (error) {
      console.error('Failed to load connections:', error);
      setToast({ message: '加载连接失败', type: 'error' });
    } finally {
      setLoading(false);
    }
  };

  const handleMount = async (id: string) => {
    setMountingId(id);
    try {
      const result = await invoke<{ success: boolean; message: string; mount_point?: string }>('mount_connection', { id });
      if (result.success) {
        setToast({ message: `挂载成功 - ${result.mount_point}`, type: 'success' });
      } else {
        setToast({ message: result.message, type: 'error' });
      }
      loadConnections();
    } catch (error) {
      console.error('Failed to mount:', error);
      const errMsg = '挂载失败：' + error;
      setToast({ message: errMsg, type: 'error' });
    } finally {
      setMountingId(null);
    }
  };

  const handleUnmount = async (id: string) => {
    try {
      await invoke('unmount_connection', { id });
      setToast({ message: '已断开连接', type: 'success' });
      loadConnections();
    } catch (error) {
      console.error('Failed to unmount:', error);
      setToast({ message: '断开失败：' + error, type: 'error' });
    }
  };

  const handleOpenFolder = async (mountPoint: string) => {
    try {
      await invoke('open_folder', { path: mountPoint + '\\' });
    } catch (error) {
      console.error('Failed to open folder:', error);
      setToast({ message: '打开文件夹失败', type: 'error' });
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm('确定要删除此连接吗？')) return;

    try {
      await invoke('remove_connection', { id });
      setToast({ message: '连接已删除', type: 'success' });
      loadConnections();
    } catch (error) {
      console.error('Failed to delete:', error);
      setToast({ message: '删除失败', type: 'error' });
    }
  };

  const handleEdit = (conn: Connection) => {
    setEditingConnection(conn);
    setShowEditModal(true);
  };

  const getHostInfo = async (id: string): Promise<string> => {
    try {
      return await invoke<string>('get_connection_host_info', { id });
    } catch {
      return '';
    }
  };

  return (
    <div className="container">
      <header className="header">
        <h1>openNetDrive</h1>
        <p className="subtitle">网络驱动器挂载工具</p>
        {mountedCount > 0 && (
          <div className="status-badge">
            已连接 {mountedCount} 个驱动器
          </div>
        )}
      </header>

      {toast && (
        <div className={`toast toast-${toast.type}`}>
          {toast.message}
        </div>
      )}

      <main className="main">
        <div className="connections-header">
          <h2>连接列表</h2>
          <button
            className="btn btn-primary"
            onClick={() => setShowAddModal(true)}
          >
            ➕ 添加连接
          </button>
        </div>

        {loading ? (
          <div className="loading-state">
            <div className="spinner"></div>
            <p>加载中...</p>
          </div>
        ) : connections.length === 0 ? (
          <div className="empty-state">
            <p>暂无连接</p>
            <p className="hint">点击"添加连接"开始使用</p>
          </div>
        ) : (
          <div className="connections-list">
            {connections.map((conn) => (
              <ConnectionCard
                key={conn.id}
                connection={conn}
                onMount={() => handleMount(conn.id)}
                onUnmount={() => handleUnmount(conn.id)}
                onOpenFolder={() => conn.mount_point && handleOpenFolder(conn.mount_point)}
                onEdit={() => handleEdit(conn)}
                onDelete={() => handleDelete(conn.id)}
                mountingId={mountingId}
                getHostInfo={() => getHostInfo(conn.id)}
              />
            ))}
          </div>
        )}
      </main>

      <footer className="footer">
        <span onClick={() => setShowSettingsModal(true)}>⚙ 设置</span>
        <span onClick={() => setShowLogs(!showLogs)}>📋 日志 {showLogs ? '▲' : '▼'}</span>
        <span onClick={() => setShowAboutModal(true)}>ℹ️ 关于</span>
      </footer>

      {showLogs && (
        <div className="log-panel">
          <div className="log-header">
            <span>日志</span>
            <button className="btn-clear" onClick={() => setLogs([])}>清空</button>
          </div>
          <div className="log-content">
            {logs.map((log, index) => (
              <div key={index} className={`log-entry log-${log.level}`}>
                <span className="log-time">{log.time}</span>
                <span className="log-level">[{log.level.toUpperCase()}]</span>
                <span className="log-message">{log.message}</span>
              </div>
            ))}
            <div ref={logEndRef} />
          </div>
        </div>
      )}

      {showAddModal && (
        <AddModal
          onClose={() => setShowAddModal(false)}
          onAdded={() => {
            loadConnections();
            setShowAddModal(false);
            setToast({ message: '连接添加成功', type: 'success' });
          }}
        />
      )}

      {showEditModal && editingConnection && (
        <EditModal
          connection={editingConnection}
          onClose={() => {
            setShowEditModal(false);
            setEditingConnection(null);
          }}
          onUpdated={() => {
            loadConnections();
            setShowEditModal(false);
            setEditingConnection(null);
            setToast({ message: '连接已更新', type: 'success' });
          }}
        />
      )}

      {showSettingsModal && (
        <SettingsModal
          onClose={() => setShowSettingsModal(false)}
          onSaved={() => {
            setShowSettingsModal(false);
            setToast({ message: '设置已保存', type: 'success' });
            loadSettings();  // 重新加载设置并应用主题
          }}
        />
      )}

      {showAboutModal && (
        <AboutModal onClose={() => setShowAboutModal(false)} />
      )}
    </div>
  );
}

interface ConnectionCardProps {
  connection: Connection;
  onMount: () => void;
  onUnmount: () => void;
  onOpenFolder: () => void;
  onEdit: () => void;
  onDelete: () => void;
  mountingId: string | null;
  getHostInfo: () => Promise<string>;
}

function ConnectionCard({ connection, onMount, onUnmount, onOpenFolder, onEdit, onDelete, mountingId, getHostInfo }: ConnectionCardProps) {
  const [hostInfo, setHostInfo] = useState<string>('');

  useEffect(() => {
    getHostInfo().then(setHostInfo);
  }, [connection.id, getHostInfo]);

  // 根据名称计算推荐的盘符
  const getSuggestedDrive = (name: string): string | null => {
    if (!name || name.trim() === '') return null;
    const firstChar = name.trim().charAt(0).toUpperCase();
    if (firstChar >= 'A' && firstChar <= 'Z') {
      return firstChar + ':';
    }
    return null;
  };

  const suggestedDrive = getSuggestedDrive(connection.name);

  return (
    <div className="connection-card">
      <div className="connection-info">
        <div className="connection-status">
          {connection.enabled ? '🟢' : '⚫'}
        </div>
        <div className="connection-details">
          <h3>{connection.name}</h3>
          <p className="connection-meta">
            <span className="meta-type">{connection.connection_type === 'webdav' ? 'WebDAV' : 'SMB'}</span>
            {connection.enabled ? (
              connection.mount_point && (
                <span className="meta-drive" style={{ color: '#4ade80' }}>本地: {connection.mount_point}</span>
              )
            ) : (
              connection.mount_point ? (
                <span className="meta-drive" style={{ color: '#fbbf24' }}>将挂载到: {connection.mount_point}</span>
              ) : (
                <span className="meta-drive" style={{ color: '#fbbf24' }}>自动: {suggestedDrive || 'Z:'}</span>
              )
            )}
            {connection.auto_mount && <span className="meta-auto">自动</span>}
          </p>
          <p className="connection-host">
            远端: {hostInfo || connection.host || '-'}
          </p>
          {connection.username && (
            <p className="connection-username" style={{ fontSize: '0.85rem', color: 'var(--text-secondary)', marginTop: '2px' }}>
              用户名: {connection.username}
            </p>
          )}
        </div>
      </div>
      <div className="connection-actions">
        {connection.enabled ? (
          <>
            <button
              className="btn btn-primary"
              onClick={onOpenFolder}
              title="打开资源管理器"
            >
              📁
            </button>
            <button
              className="btn btn-danger"
              onClick={onUnmount}
              disabled={mountingId === connection.id}
            >
              断开
            </button>
          </>
        ) : (
          <button
            className="btn btn-success"
            onClick={onMount}
            disabled={mountingId === connection.id}
          >
            {mountingId === connection.id ? '...' : '连接'}
          </button>
        )}
        <button className="btn btn-secondary" onClick={onEdit}>
          编辑
        </button>
        <button className="btn btn-secondary" onClick={onDelete}>
          删除
        </button>
      </div>
    </div>
  );
}

interface AddModalProps {
  onClose: () => void;
  onAdded: () => void;
}

function AddModal({ onClose, onAdded }: AddModalProps) {
  const [name, setName] = useState('');
  const [type, setType] = useState('smb');
  const [host, setHost] = useState('');
  const [share, setShare] = useState('');        // SMB 共享名称
  const [remotePath, setRemotePath] = useState('/'); // SMB 远程路径
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [capsLockOn, setCapsLockOn] = useState(false);
  const [autoMount, setAutoMount] = useState(false);
  const [mountPoint, setMountPoint] = useState('');  // 空字符串表示自动分配
  const [availableDrives, setAvailableDrives] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [hostValidation, setHostValidation] = useState<{isValid: boolean; message: string; type: 'ip' | 'domain' | 'empty'}>({isValid: true, message: '', type: 'empty'});

  useEffect(() => {
    loadAvailableDrives();
  }, []);

  // 协议类型切换时清空相关字段
  useEffect(() => {
    if (type === 'webdav') {
      setShare('');
      setRemotePath('/');
    }
  }, [type]);

  // 根据名称计算推荐的盘符
  const getSuggestedDrive = (connectionName: string, availableDrives: string[]): string | null => {
    if (!connectionName || connectionName.trim() === '') return null;
    const firstChar = connectionName.trim().charAt(0).toUpperCase();
    if (firstChar >= 'A' && firstChar <= 'Z') {
      const drive = firstChar + ':';
      if (availableDrives.includes(drive)) {
        return drive;
      }
    }
    return null;
  };

  // 验证服务器地址（IP或域名）
  const validateHost = (value: string) => {
    if (!value || value.trim() === '') {
      return { isValid: false, message: '', type: 'empty' as const };
    }

    const trimmed = value.trim();

    // 检查是否为IP地址（简单检查：4组数字，用.分隔）
    const ipPattern = /^(\d{1,3}\.){3}\d{1,3}$/;
    if (ipPattern.test(trimmed)) {
      const parts = trimmed.split('.');
      // 检查每个部分是否在0-255范围内
      const isValidIP = parts.every(part => {
        const num = parseInt(part, 10);
        return num >= 0 && num <= 255;
      });
      if (isValidIP) {
        return { isValid: true, message: 'IP地址', type: 'ip' as const };
      } else {
        return { isValid: false, message: '无效的IP地址', type: 'ip' as const };
      }
    }

    // 检查是否为域名（字母、数字、点、-，至少有一个点）
    const domainPattern = /^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?)+$/;
    if (domainPattern.test(trimmed)) {
      return { isValid: true, message: '域名', type: 'domain' as const };
    }

    // 简单域名检查（没有点但包含字母）
    const simpleDomainPattern = /^[a-zA-Z][a-zA-Z0-9-]*$/;
    if (simpleDomainPattern.test(trimmed)) {
      return { isValid: true, message: '计算机名', type: 'domain' as const };
    }

    return { isValid: false, message: '请输入有效的IP地址或域名', type: 'domain' as const };
  };

  // 处理服务器地址变化
  const handleHostChange = (value: string) => {
    setHost(value);
    const validation = validateHost(value);
    setHostValidation(validation);
  };

  const loadAvailableDrives = async () => {
    try {
      const drives = await invoke<string[]>('get_available_drives');
      setAvailableDrives(drives);
      if (drives.length > 0) {
        // 选择第一个未使用的盘符后面的字母
        setMountPoint(drives[drives.length - 1] || 'Z:');
      }
    } catch (e) {
      console.error('Failed to get drives:', e);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // 验证服务器地址
    if (type === 'smb' && host && !hostValidation.isValid) {
      alert('请输入有效的服务器地址（IP地址或域名）');
      return;
    }

    setSubmitting(true);

    try {
      await invoke('add_connection', {
        name,
        connectionType: type,
        host,
        share: share || null,
        remotePath: remotePath || null,
        username,
        password,
        autoMount,
        mountPoint: mountPoint || null,
      });
      onAdded();
    } catch (error) {
      console.error('Failed to add connection:', error);
      alert('添加失败：' + error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>➕ 添加连接</h2>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label>名称</label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：我的 NAS"
              required
            />
          </div>

          {/* 协议类型和挂载盘符在同一行 */}
          <div className="form-row">
            <div className="form-group">
              <label>协议类型</label>
              <select value={type} onChange={(e) => setType(e.target.value)}>
                <option value="webdav">WebDAV</option>
                <option value="smb">SMB/CIFS (Windows文件共享)</option>
              </select>
            </div>
            <div className="form-group">
              <label>挂载盘符</label>
              <select value={mountPoint} onChange={(e) => setMountPoint(e.target.value)}>
                <option value="">自动分配 {getSuggestedDrive(name, availableDrives) ? `(将使用 ${getSuggestedDrive(name, availableDrives)})` : ''}</option>
                {availableDrives.map(drive => (
                  <option key={drive} value={drive}>{drive} (可用)</option>
                ))}
              </select>
            </div>
          </div>

          {type === 'smb' ? (
            <>
              {/* SMB 模式：主机和共享在同一行 */}
              <div className="form-row">
                <div className="form-group">
                  <label>服务器地址或域名</label>
                  <input
                    type="text"
                    value={host}
                    onChange={(e) => handleHostChange(e.target.value)}
                    placeholder="192.168.1.100 或 nas.example.com"
                    required
                    style={{ borderColor: host && !hostValidation.isValid ? '#ff4d4f' : undefined }}
                  />
                  {host && (
                    <div style={{
                      fontSize: '12px',
                      marginTop: '4px',
                      color: hostValidation.isValid ? (hostValidation.type === 'ip' ? '#1890ff' : '#52c41a') : '#ff4d4f'
                    }}>
                      {hostValidation.isValid ? `✓ ${hostValidation.message}` : `✗ ${hostValidation.message}`}
                    </div>
                  )}
                </div>
                <div className="form-group">
                  <label>共享名称</label>
                  <input
                    type="text"
                    value={share}
                    onChange={(e) => setShare(e.target.value)}
                    placeholder="sharedfolder"
                    required
                  />
                </div>
              </div>

              <div className="form-group">
                <label>远程路径 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
                <input
                  type="text"
                  value={remotePath}
                  onChange={(e) => setRemotePath(e.target.value)}
                  placeholder="例: /documents 或 /"
                />
                {/* 实时显示 UNC 路径预览 */}
                {type === 'smb' && host && (
                  <div style={{
                    marginTop: '8px',
                    padding: '8px 12px',
                    background: 'var(--bg-primary)',
                    borderRadius: '4px',
                    fontSize: '0.8rem',
                    fontFamily: 'monospace',
                    color: share ? 'var(--accent)' : 'var(--text-secondary)'
                  }}>
                    📍 预览: {share ? (() => {
                      const cleanPath = remotePath.replace(/^\/+/, '').replace(/\//g, '\\');
                      return cleanPath ? `\\\\${host}\\${share}\\${cleanPath}` : `\\\\${host}\\${share}`;
                    })() : `\\\\${host}\\<共享名称>`}
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className="form-group">
              <label>WebDAV URL</label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="https://example.com/dav"
                required
              />
            </div>
          )}

          {/* 用户名密码在同一行 */}
          <div className="form-row">
            <div className="form-group">
              <label>用户名 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="可选"
              />
            </div>
            <div className="form-group">
              <label>密码 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyUp={(e) => {
                  setCapsLockOn(e.getModifierState('CapsLock'));
                }}
                placeholder="可选"
              />
              {capsLockOn && (
                <div style={{ fontSize: '12px', color: '#faad14', marginTop: '4px' }}>
                  ⚠️ 大写锁定已开启
                </div>
              )}
            </div>
          </div>

          <div className="form-group checkbox-group">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={autoMount}
                onChange={(e) => setAutoMount(e.target.checked)}
              />
              <span>启动时自动挂载</span>
            </label>
          </div>

          <div className="modal-actions">
            <button type="button" className="btn btn-secondary" onClick={onClose} disabled={submitting}>
              取消
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? '添加中...' : '添加'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

interface EditModalProps {
  connection: Connection;
  onClose: () => void;
  onUpdated: () => void;
}

function EditModal({ connection, onClose, onUpdated }: EditModalProps) {
  const [name, setName] = useState(connection.name);
  const [type, setType] = useState(connection.connection_type);
  const [host, setHost] = useState(connection.host || '');
  const [share, setShare] = useState(connection.share || '');         // SMB 共享名称
  const [remotePath, setRemotePath] = useState(connection.remote_path || '/'); // SMB 远程路径
  const [username, setUsername] = useState(connection.username || '');
  const [mountPoint, setMountPoint] = useState(connection.mount_point || '');
  const [autoMount, setAutoMount] = useState(connection.auto_mount);
  const [password, setPassword] = useState('');
  const [capsLockOn, setCapsLockOn] = useState(false);
  const [availableDrives, setAvailableDrives] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [hostValidation, setHostValidation] = useState<{isValid: boolean; message: string; type: 'ip' | 'domain' | 'empty'}>({isValid: true, message: '', type: 'empty'});

  // 根据名称计算推荐的盘符
  const getSuggestedDrive = (connectionName: string, drives: string[]): string | null => {
    if (!connectionName || connectionName.trim() === '') return null;
    const firstChar = connectionName.trim().charAt(0).toUpperCase();
    if (firstChar >= 'A' && firstChar <= 'Z') {
      const drive = firstChar + ':';
      if (drives.includes(drive)) {
        return drive;
      }
    }
    return null;
  };

  // 验证服务器地址（IP或域名）
  const validateHost = (value: string) => {
    if (!value || value.trim() === '') {
      return { isValid: false, message: '', type: 'empty' as const };
    }

    const trimmed = value.trim();

    // 检查是否为IP地址（简单检查：4组数字，用.分隔）
    const ipPattern = /^(\d{1,3}\.){3}\d{1,3}$/;
    if (ipPattern.test(trimmed)) {
      const parts = trimmed.split('.');
      // 检查每个部分是否在0-255范围内
      const isValidIP = parts.every(part => {
        const num = parseInt(part, 10);
        return num >= 0 && num <= 255;
      });
      if (isValidIP) {
        return { isValid: true, message: 'IP地址', type: 'ip' as const };
      } else {
        return { isValid: false, message: '无效的IP地址', type: 'ip' as const };
      }
    }

    // 检查是否为域名（字母、数字、点、-，至少有一个点）
    const domainPattern = /^[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]*[a-zA-Z0-9])?)+$/;
    if (domainPattern.test(trimmed)) {
      return { isValid: true, message: '域名', type: 'domain' as const };
    }

    // 简单域名检查（没有点但包含字母）
    const simpleDomainPattern = /^[a-zA-Z][a-zA-Z0-9-]*$/;
    if (simpleDomainPattern.test(trimmed)) {
      return { isValid: true, message: '计算机名', type: 'domain' as const };
    }

    return { isValid: false, message: '请输入有效的IP地址或域名', type: 'domain' as const };
  };

  // 处理服务器地址变化
  const handleHostChange = (value: string) => {
    setHost(value);
    const validation = validateHost(value);
    setHostValidation(validation);
  };

  // 初始化时验证一次host
  useEffect(() => {
    if (host) {
      const validation = validateHost(host);
      setHostValidation(validation);
    }
  }, []);

  useEffect(() => {
    loadAvailableDrives();
  }, []);

  const loadAvailableDrives = async () => {
    try {
      const drives = await invoke<string[]>('get_available_drives');
      setAvailableDrives(drives);
    } catch (e) {
      console.error('Failed to get drives:', e);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // 验证服务器地址
    if (type === 'smb' && host && !hostValidation.isValid) {
      alert('请输入有效的服务器地址（IP地址或域名）');
      return;
    }

    setSubmitting(true);

    try {
      await invoke('update_connection_full', {
        id: connection.id,
        name,
        connectionType: type,
        host,
        share: share || null,
        remotePath: remotePath || null,
        username: username,
        password: password || null,
        mountPoint: mountPoint || null,
        autoMount,
      });
      onUpdated();
    } catch (error) {
      console.error('Failed to update connection:', error);
      alert('更新失败：' + error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>✏️ 编辑连接</h2>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label>名称</label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>

          {/* 协议类型和挂载盘符在同一行 */}
          <div className="form-row">
            <div className="form-group">
              <label>协议类型</label>
              <select value={type} onChange={(e) => setType(e.target.value)}>
                <option value="webdav">WebDAV</option>
                <option value="smb">SMB/CIFS (Windows文件共享)</option>
              </select>
            </div>
            <div className="form-group">
              <label>挂载盘符</label>
              <select value={mountPoint} onChange={(e) => setMountPoint(e.target.value)}>
                <option value="">自动分配 {getSuggestedDrive(name, availableDrives) ? `(将使用 ${getSuggestedDrive(name, availableDrives)})` : ''}</option>
                {availableDrives.map(drive => (
                  <option key={drive} value={drive}>{drive} (可用)</option>
                ))}
                {connection.mount_point && !availableDrives.includes(connection.mount_point) && (
                  <option value={connection.mount_point}>{connection.mount_point} (当前)</option>
                )}
              </select>
            </div>
          </div>

          {type === 'smb' ? (
            <>
              {/* SMB 模式：主机和共享在同一行 */}
              <div className="form-row">
                <div className="form-group">
                  <label>服务器地址或域名</label>
                  <input
                    type="text"
                    value={host}
                    onChange={(e) => handleHostChange(e.target.value)}
                    placeholder="192.168.1.100 或 nas.example.com"
                    style={{ borderColor: host && !hostValidation.isValid ? '#ff4d4f' : undefined }}
                  />
                  {host && (
                    <div style={{
                      fontSize: '12px',
                      marginTop: '4px',
                      color: hostValidation.isValid ? (hostValidation.type === 'ip' ? '#1890ff' : '#52c41a') : '#ff4d4f'
                    }}>
                      {hostValidation.isValid ? `✓ ${hostValidation.message}` : `✗ ${hostValidation.message}`}
                    </div>
                  )}
                </div>
                <div className="form-group">
                  <label>共享名称</label>
                  <input
                    type="text"
                    value={share}
                    onChange={(e) => setShare(e.target.value)}
                    placeholder="sharedfolder"
                  />
                </div>
              </div>

              <div className="form-group">
                <label>远程路径 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
                <input
                  type="text"
                  value={remotePath}
                  onChange={(e) => setRemotePath(e.target.value)}
                  placeholder="例: /documents 或 /"
                />
              </div>
            </>
          ) : (
            <div className="form-group">
              <label>WebDAV URL</label>
              <input
                type="text"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="https://example.com/dav"
              />
            </div>
          )}

          {/* 用户名和密码在同一行 */}
          <div className="form-row">
            <div className="form-group">
              <label>用户名 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="用户名"
              />
            </div>
            <div className="form-group">
              <label>密码 <span style={{opacity: 0.6, fontWeight: 'normal'}}>(可选)</span></label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyUp={(e) => {
                  setCapsLockOn(e.getModifierState('CapsLock'));
                }}
                placeholder="留空则不更改"
              />
              {capsLockOn && (
                <div style={{ fontSize: '12px', color: '#faad14', marginTop: '4px' }}>
                  ⚠️ 大写锁定已开启
                </div>
              )}
            </div>
          </div>

          <div className="form-group checkbox-group">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={autoMount}
                onChange={(e) => setAutoMount(e.target.checked)}
              />
              <span>启动时自动挂载</span>
            </label>
          </div>

          <div className="modal-actions">
            <button type="button" className="btn btn-secondary" onClick={onClose} disabled={submitting}>
              取消
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? '保存中...' : '保存'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

interface SettingsModalProps {
  onClose: () => void;
  onSaved: () => void;
}

function SettingsModal({ onClose, onSaved }: SettingsModalProps) {
  const [themeMode, setThemeMode] = useState('system');
  const [autoStart, setAutoStart] = useState(false);
  const [logLevel, setLogLevel] = useState('info');
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const settings = await invoke<AppSettings>('get_settings');
      setThemeMode(settings.theme_mode || 'system');
      setAutoStart(settings.auto_start);
      setLogLevel(settings.log_level);
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);

    try {
      await invoke('save_settings', {
        settings: {
          theme_mode: themeMode,
          start_minimized: false,
          auto_start: autoStart,
          log_level: logLevel,
        },
      });
      onSaved();
    } catch (error) {
      console.error('Failed to save settings:', error);
      alert('保存失败：' + error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>⚙️ 设置</h2>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label>主题模式</label>
            <div style={{ display: 'flex', gap: '16px', marginTop: '8px' }}>
              <label className="checkbox-label">
                <input
                  type="radio"
                  name="themeMode"
                  value="light"
                  checked={themeMode === 'light'}
                  onChange={(e) => setThemeMode(e.target.value)}
                />
                <span>浅色</span>
              </label>
              <label className="checkbox-label">
                <input
                  type="radio"
                  name="themeMode"
                  value="dark"
                  checked={themeMode === 'dark'}
                  onChange={(e) => setThemeMode(e.target.value)}
                />
                <span>深色</span>
              </label>
              <label className="checkbox-label">
                <input
                  type="radio"
                  name="themeMode"
                  value="system"
                  checked={themeMode === 'system'}
                  onChange={(e) => setThemeMode(e.target.value)}
                />
                <span>跟随系统</span>
              </label>
            </div>
          </div>

          <div className="form-group checkbox-group">
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={autoStart}
                onChange={(e) => setAutoStart(e.target.checked)}
              />
              <span>开机自动启动</span>
            </label>
          </div>

          <div className="form-group">
            <label>日志级别</label>
            <select value={logLevel} onChange={(e) => setLogLevel(e.target.value)}>
              <option value="debug">Debug</option>
              <option value="info">Info</option>
              <option value="warn">Warning</option>
              <option value="error">Error</option>
            </select>
          </div>

          <div className="modal-actions">
            <button type="button" className="btn btn-secondary" onClick={onClose} disabled={submitting}>
              取消
            </button>
            <button type="submit" className="btn btn-primary" disabled={submitting}>
              {submitting ? '保存中...' : '保存'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function AboutModal({ onClose }: { onClose: () => void }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>ℹ️ 关于 openNetDrive</h2>
        <div className="about-content">
          <p><strong>版本:</strong> 0.1.0</p>
          <p><strong>描述:</strong> 跨平台的网络驱动器挂载工具</p>
          <p>支持通过 WebDAV/SMB 协议将 NAS 共享文件夹映射为本地磁盘。</p>
          <hr />
          <p className="copyright">基于 Tauri + React + Rust 构建</p>
          <p className="copyright">采用 GPL-3.0 协议开源</p>
        </div>
        <div className="modal-actions">
          <button type="button" className="btn btn-primary" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}

export default App;
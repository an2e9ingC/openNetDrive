import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface Connection {
  id: string;
  name: string;
  connection_type: string;
  mount_point: string | null;
  auto_mount: boolean;
  enabled: boolean;
  host?: string;
  username?: string;
}

interface Toast {
  message: string;
  type: 'success' | 'error' | 'info';
}

function App() {
  const [connections, setConnections] = useState<Connection[]>([]);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showEditModal, setShowEditModal] = useState(false);
  const [editingConnection, setEditingConnection] = useState<Connection | null>(null);
  const [loading, setLoading] = useState(true);
  const [toast, setToast] = useState<Toast | null>(null);
  const [mountingId, setMountingId] = useState<string | null>(null);

  useEffect(() => {
    loadConnections();
  }, []);

  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [toast]);

  const loadConnections = async () => {
    setLoading(true);
    try {
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
      await invoke('mount_connection', { id });
      setToast({ message: '挂载成功', type: 'success' });
      loadConnections();
    } catch (error) {
      console.error('Failed to mount:', error);
      setToast({ message: '挂载失败：' + error, type: 'error' });
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

  return (
    <div className="container">
      <header className="header">
        <h1>openNetDrive</h1>
        <p className="subtitle">网络驱动器挂载工具</p>
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
              <div key={conn.id} className="connection-card">
                <div className="connection-info">
                  <div className="connection-status">
                    {conn.enabled ? '🟢' : '⚫'}
                  </div>
                  <div className="connection-details">
                    <h3>{conn.name}</h3>
                    <p className="connection-meta">
                      {conn.connection_type} • {conn.mount_point || '未挂载'}
                      {conn.auto_mount && ' • 自动挂载'}
                    </p>
                  </div>
                </div>
                <div className="connection-actions">
                  {conn.enabled ? (
                    <button
                      className="btn btn-danger"
                      onClick={() => handleUnmount(conn.id)}
                      disabled={mountingId === conn.id}
                    >
                      断开
                    </button>
                  ) : (
                    <button
                      className="btn btn-success"
                      onClick={() => handleMount(conn.id)}
                      disabled={mountingId === conn.id}
                    >
                      {mountingId === conn.id ? '...' : '连接'}
                    </button>
                  )}
                  <button
                    className="btn btn-secondary"
                    onClick={() => handleEdit(conn)}
                  >
                    编辑
                  </button>
                  <button
                    className="btn btn-secondary"
                    onClick={() => handleDelete(conn.id)}
                  >
                    删除
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </main>

      <footer className="footer">
        <span>⚙ 设置</span>
        <span>📊 日志</span>
        <span>ℹ️ 关于</span>
      </footer>

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
    </div>
  );
}

interface AddModalProps {
  onClose: () => void;
  onAdded: () => void;
}

function AddModal({ onClose, onAdded }: AddModalProps) {
  const [name, setName] = useState('');
  const [type, setType] = useState('webdav');
  const [host, setHost] = useState('');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [autoMount, setAutoMount] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);

    try {
      await invoke('add_connection', {
        name,
        connectionType: type,
        host,
        username,
        password,
        autoMount,
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

          <div className="form-group">
            <label>协议类型</label>
            <select value={type} onChange={(e) => setType(e.target.value)}>
              <option value="webdav">WebDAV</option>
              <option value="smb">SMB/CIFS</option>
            </select>
          </div>

          <div className="form-group">
            <label>{type === 'webdav' ? 'URL' : '主机地址'}</label>
            <input
              type="text"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder={type === 'webdav' ? 'https://example.com/dav' : '192.168.1.100'}
              required
            />
          </div>

          <div className="form-group">
            <label>用户名</label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="可选"
            />
          </div>

          <div className="form-group">
            <label>密码</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="可选"
            />
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
  const [autoMount, setAutoMount] = useState(connection.auto_mount);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);

    try {
      await invoke('update_connection', {
        id: connection.id,
        name,
        connectionType: type,
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

          <div className="form-group">
            <label>协议类型</label>
            <select value={type} onChange={(e) => setType(e.target.value)}>
              <option value="webdav">WebDAV</option>
              <option value="smb">SMB/CIFS</option>
            </select>
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

export default App;

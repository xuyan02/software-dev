import '@xyflow/react/dist/style.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Editor, BASE } from './Editor';
import { VersionView } from './VersionView';
import './styles.css';

function segments(): string[] {
  const path = window.location.pathname;
  const rest = path.startsWith(BASE) ? path.slice(BASE.length) : path;
  return rest
    .split('/')
    .filter(Boolean)
    .map((segment) => decodeURIComponent(segment));
}

function App() {
  const parts = segments();
  if (parts.length === 1) {
    return <Editor workflow={parts[0]} />;
  }
  if (parts.length === 3 && parts[1] === 'versions') {
    const version = Number(parts[2]);
    if (Number.isInteger(version) && version > 0) {
      return <VersionView workflow={parts[0]} version={version} />;
    }
  }
  return (
    <div className="full-screen">
      <p>没有这个页面。</p>
      <a href="/workflows">返回流程列表</a>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

const features = [
  ["Clipboard", "Share text and links between paired devices"],
  ["File transfer", "Send files directly over your local network"],
  ["Device controls", "Use trusted devices as connected companions"]
];
export default function App() {
  return <main className="shell">
    <aside className="sidebar"><div className="brand"><span className="mark">B</span> BRIDGE</div><p className="nav-label">WORKSPACE</p><div className="nav active">◈　Overview</div><div className="nav">⌁　Devices</div><div className="nav">⇧　Transfers</div><div className="side-foot">●　Local-first by design</div></aside>
    <section className="content">
      <header><div><p className="eyebrow">YOUR ECOSYSTEM</p><h1>Good to see you.</h1><p className="muted">Your devices, connected when you need them.</p></div><div className="avatar">B</div></header>
      <section className="hero"><div className="hero-copy"><p className="eyebrow accent">BRIDGE NETWORK</p><h2>One ecosystem.<br/>Every device.</h2><p>Bring your Windows PC, Android phone and iPhone together in one private workspace.</p><button disabled>Pair a device　↗</button></div><div className="orbit"><div className="ring r1"/><div className="ring r2"/><div className="core">B</div><span className="node n1">▣</span><span className="node n2">▤</span><span className="node n3">▧</span></div></section>
      <div className="section-head"><div><p className="eyebrow">OVERVIEW</p><h3>Your workspace</h3></div><span className="pill">FOUNDATION BUILD</span></div>
      <section className="stats"><article><b>⌘</b><strong>0</strong><span>Paired devices</span></article><article><b>⇄</b><strong>0</strong><span>Active transfers</span></article><article><b>◉</b><strong>Local</strong><span>Network mode</span></article></section>
      <div className="section-head"><div><p className="eyebrow">CAPABILITIES</p><h3>Made to work together</h3></div></div>
      <section className="features">{features.map(([name, detail]) => <article className="feature" key={name}><div><h4>{name}</h4><p>{detail}</p></div><span>PLANNED</span></article>)}</section>
      <footer>Early foundation build. Device discovery, pairing and transfers are not active yet.</footer>
    </section>
  </main>;
}

# Interactive HTML Visualization Guide

The HTML exporter generates a fully interactive, self-contained visualization of your Surv IR project using D3.js.

## Quick Start

```bash
# Generate interactive HTML
surc export html examples/surv.toml > architecture.html

# Open in browser
open architecture.html  # macOS
xdg-open architecture.html  # Linux
start architecture.html  # Windows
```

## Features

### 🎯 Interactive Graph

- **Force-directed layout** - Nodes automatically arrange themselves
- **Drag nodes** - Click and drag to reposition
- **Zoom & Pan** - Mouse wheel to zoom, drag background to pan
- **Physics simulation** - Real-time layout updates

### 🔍 Node Details

Click any node to view:
- **Type & metadata** (kind, role, intent, purpose)
- **Input/Output schemas** for functions
- **Fields** for schemas
- **Related connections** highlighted

### 🎨 Visual Encoding

**Node Colors:**
- 🔵 Blue - Schemas (node)
- 🟢 Green - Functions
- 🟣 Purple - Modules
- 🟡 Yellow - Boundary schemas
- 🟣 Purple - Space schemas

**Links:**
- Solid arrows - Direct relationships
- Dashed arrows - Soft dependencies

### 🔎 Search & Filter

**Search Box:**
- Type to filter nodes by name
- Real-time highlighting

**Node Type Filters:**
- Toggle Schemas on/off
- Toggle Functions on/off
- Toggle Modules on/off

**Schema Type Filters:**
- Node
- Edge
- Boundary
- Space

**Link Type Filters:**
- Function I/O (input/output relationships)
- Module Uses (mod → schema/func)
- Module Requires (mod → mod dependencies)
- Schema Relations (edge/boundary/space)

### 🎛️ Controls

**Reset View** - Reset zoom to default
**Reset Layout** - Restart physics simulation

## Example Output

The generated HTML file contains:
- ✅ Complete visualization code
- ✅ D3.js library (loaded from CDN)
- ✅ All project data embedded as JSON
- ✅ Dark theme optimized UI
- ✅ No external dependencies (except D3.js CDN)

File size: ~25KB for a typical project

## How It Works

### Data Structure

The exporter converts your Surv IR into a graph structure:

```json
{
  "nodes": [
    {
      "id": "schema.user",
      "label": "user",
      "type": "schema",
      "group": "node",
      "metadata": {
        "kind": "node",
        "role": "data",
        "fields": {"user_id": "string", "name": "string"}
      }
    }
  ],
  "links": [
    {
      "source": "func.create_user",
      "target": "schema.user",
      "label": "output",
      "type": "func_output"
    }
  ]
}
```

### Interaction Patterns

1. **Click node** → Show detail panel + highlight connections
2. **Click background** → Clear selection
3. **Drag node** → Reposition (temporary, resets on layout restart)
4. **Search** → Dim non-matching nodes
5. **Uncheck filter** → Hide filtered nodes/links

## Use Cases

### 1. Architecture Documentation

```bash
surc export html surv.toml > docs/architecture.html
git add docs/architecture.html
git commit -m "Add interactive architecture diagram"
```

View in GitHub Pages or any static hosting.

### 2. Code Review

```bash
# Generate before/after visualizations
git show main:surv.toml | surc export html - > old.html
surc export html surv.toml > new.html

# Compare visually in browser
```

### 3. Onboarding

Share the HTML file with new team members to explore:
- System structure
- Module dependencies
- Data flow patterns

### 4. Debugging

- Find unused schemas/funcs (isolated nodes)
- Identify circular dependencies (module requires)
- Trace data flow through pipelines

## Customization

The generated HTML is self-contained and can be edited:

### Change Colors

Edit the `colorMap` object in the JavaScript:

```javascript
const colorMap = {
    'node': '#YOUR_COLOR',
    'function': '#YOUR_COLOR',
    'module': '#YOUR_COLOR'
};
```

### Adjust Physics

Modify force simulation parameters:

```javascript
const simulation = d3.forceSimulation(data.nodes)
    .force('link', d3.forceLink(data.links).distance(200))  // link distance
    .force('charge', d3.forceManyBody().strength(-500))     // repulsion
    .force('center', d3.forceCenter(width / 2, height / 2))
    .force('collision', d3.forceCollide().radius(50));      // collision radius
```

### Light Theme

Change CSS variables:

```css
body {
    background: #ffffff;
    color: #1a1a1a;
}

#sidebar {
    background: #f5f5f5;
    border-right: 1px solid #ddd;
}
```

## Browser Compatibility

- ✅ Chrome/Edge 90+
- ✅ Firefox 88+
- ✅ Safari 14+

Requires JavaScript enabled.

## Performance

- **Small projects** (<50 nodes): Smooth
- **Medium projects** (50-200 nodes): Good
- **Large projects** (200+ nodes): Consider filtering or splitting into multiple files

## Tips

1. **Start with filters** - Hide node types you don't need initially
2. **Use search** - Quickly locate specific schemas/funcs
3. **Save snapshots** - Take screenshots of interesting views
4. **Embed in docs** - Works great in static site generators
5. **Version control** - Commit HTML to track architecture evolution

## Comparison with Mermaid

| Feature | HTML Export | Mermaid Export |
|---------|-------------|----------------|
| Interactivity | ✅ Full | ❌ Static |
| Search | ✅ | ❌ |
| Filter | ✅ | ❌ |
| File size | ~25KB | ~5KB |
| GitHub native | ❌ | ✅ |
| Offline | ⚠️ Needs D3 CDN | ✅ |
| Customizable | ✅ Easily | ⚠️ Limited |

Use **Mermaid** for:
- GitHub README/Wiki
- Quick diagrams
- Documentation

Use **HTML** for:
- Exploration & debugging
- Large projects
- Interactive demos
- Detailed analysis

## Troubleshooting

### Graph appears empty
- Check browser console for errors
- Verify D3.js CDN is accessible
- Check that nodes have valid coordinates

### Nodes overlap too much
- Click "Reset Layout" to restart simulation
- Increase repulsion strength (see Customization)

### Can't see all nodes
- Use "Reset View" button
- Check filters are not hiding nodes
- Zoom out (mouse wheel)

### Performance is slow
- Reduce number of nodes with filters
- Consider splitting project into packages
- Use static Mermaid export for very large graphs

## Future Enhancements

Potential additions:
- Save/load layout positions (localStorage)
- Export to PNG/SVG
- Mini-map for large graphs
- Cluster by package/namespace
- Timeline view (git history)
- Compare mode (diff two versions)

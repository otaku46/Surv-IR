use crate::ast::Section;
use crate::deploy::ast::DeployFile;
use crate::project::ProjectAST;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct Node {
    id: String,
    label: String,
    #[serde(rename = "type")]
    node_type: String,
    group: String,
    metadata: NodeMetadata,
}

#[derive(Serialize)]
struct NodeMetadata {
    kind: String,
    role: Option<String>,
    intent: Option<String>,
    purpose: Option<String>,
    fields: HashMap<String, String>,
    input: Vec<String>,
    output: Vec<String>,
}

#[derive(Serialize)]
struct Link {
    source: String,
    target: String,
    label: String,
    #[serde(rename = "type")]
    link_type: String,
}

#[derive(Serialize)]
struct GraphData {
    nodes: Vec<Node>,
    links: Vec<Link>,
}

// Deploy IR specific structures
#[derive(Serialize)]
struct DeployNode {
    id: String,
    label: String,
    #[serde(rename = "type")]
    node_type: String,
    group: String,
    metadata: DeployNodeMetadata,
}

#[derive(Serialize)]
struct DeployNodeMetadata {
    target: Option<String>,
    target_kind: Option<String>,
    secrets: Vec<String>,
    permissions: Option<String>,
    artifacts: Vec<String>,
    side_effects: Vec<String>,
    commands: Vec<String>,
}

#[derive(Serialize)]
struct DeployGraphData {
    nodes: Vec<DeployNode>,
    links: Vec<Link>,
    pipeline: Option<PipelineInfo>,
    targets: Vec<TargetInfo>,
}

#[derive(Serialize)]
struct PipelineInfo {
    name: String,
    description: String,
}

#[derive(Serialize)]
struct TargetInfo {
    name: String,
    kind: String,
    domain: String,
}

pub struct HtmlExporter;

impl HtmlExporter {
    pub fn new() -> Self {
        Self
    }

    pub fn export_deploy_interactive(&self, deploy: &DeployFile) -> String {
        let graph_data = self.build_deploy_graph_data(deploy);
        let graph_json = serde_json::to_string_pretty(&graph_data).unwrap();

        self.generate_deploy_html(&graph_json)
    }

    fn build_deploy_graph_data(&self, deploy: &DeployFile) -> DeployGraphData {
        let mut nodes = Vec::new();
        let mut links = Vec::new();

        // Build job nodes
        for (job_name, job) in &deploy.jobs {
            let target_kind = if !job.uses_target.is_empty() {
                let target_name = job.uses_target.strip_prefix("target.").unwrap_or(&job.uses_target);
                deploy.targets.get(target_name).map(|t| t.kind.clone())
            } else {
                None
            };

            nodes.push(DeployNode {
                id: format!("job.{}", job_name),
                label: job_name.clone(),
                node_type: "job".to_string(),
                group: target_kind.clone().unwrap_or_else(|| "default".to_string()),
                metadata: DeployNodeMetadata {
                    target: if job.uses_target.is_empty() {
                        None
                    } else {
                        Some(job.uses_target.clone())
                    },
                    target_kind,
                    secrets: job.needs_secrets.clone(),
                    permissions: if job.uses_perm.is_empty() {
                        None
                    } else {
                        Some(job.uses_perm.clone())
                    },
                    artifacts: job.produces.clone(),
                    side_effects: job.side_effects.clone(),
                    commands: job.runs.clone(),
                },
            });

            // Build dependency links
            for req in &job.requires {
                let req_name = req.strip_prefix("job.").unwrap_or(req);
                links.push(Link {
                    source: format!("job.{}", req_name),
                    target: format!("job.{}", job_name),
                    label: "requires".to_string(),
                    link_type: "dependency".to_string(),
                });
            }
        }

        // Extract pipeline info
        let pipeline = deploy.pipeline.as_ref().map(|p| PipelineInfo {
            name: p.name.clone(),
            description: p.description.clone(),
        });

        // Extract target info
        let targets = deploy
            .targets
            .iter()
            .map(|(name, target)| TargetInfo {
                name: name.clone(),
                kind: target.kind.clone(),
                domain: target.domain.clone(),
            })
            .collect();

        DeployGraphData {
            nodes,
            links,
            pipeline,
            targets,
        }
    }

    pub fn export_interactive(&self, project: &ProjectAST) -> String {
        let graph_data = self.build_graph_data(project);
        let graph_json = serde_json::to_string_pretty(&graph_data).unwrap();

        self.generate_html(&graph_json)
    }

    fn build_graph_data(&self, project: &ProjectAST) -> GraphData {
        let mut nodes = Vec::new();
        let mut links = Vec::new();

        // Collect schemas
        for (_file_path, file) in &project.files {
            for section in &file.sections {
                match section {
                    Section::Schema(schema) => {
                        nodes.push(Node {
                            id: format!("schema.{}", schema.name),
                            label: schema.name.clone(),
                            node_type: "schema".to_string(),
                            group: schema.kind.clone(),
                            metadata: NodeMetadata {
                                kind: schema.kind.clone(),
                                role: Some(schema.role.clone()),
                                intent: None,
                                purpose: None,
                                fields: schema.fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                                input: Vec::new(),
                                output: Vec::new(),
                            },
                        });

                        // Add schema relationships
                        match schema.kind.as_str() {
                            "edge" => {
                                if !schema.from.is_empty() {
                                    links.push(Link {
                                        source: schema.from.clone(),
                                        target: format!("schema.{}", schema.name),
                                        label: "from".to_string(),
                                        link_type: "edge_from".to_string(),
                                    });
                                }
                                if !schema.to.is_empty() {
                                    links.push(Link {
                                        source: format!("schema.{}", schema.name),
                                        target: schema.to.clone(),
                                        label: "to".to_string(),
                                        link_type: "edge_to".to_string(),
                                    });
                                }
                            }
                            "boundary" => {
                                for over_schema in &schema.over {
                                    links.push(Link {
                                        source: format!("schema.{}", schema.name),
                                        target: over_schema.clone(),
                                        label: "contains".to_string(),
                                        link_type: "boundary".to_string(),
                                    });
                                }
                            }
                            "space" => {
                                if !schema.base.is_empty() {
                                    links.push(Link {
                                        source: format!("schema.{}", schema.name),
                                        target: schema.base.clone(),
                                        label: "based on".to_string(),
                                        link_type: "space".to_string(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    Section::Func(func) => {
                        nodes.push(Node {
                            id: format!("func.{}", func.name),
                            label: func.name.clone(),
                            node_type: "func".to_string(),
                            group: "function".to_string(),
                            metadata: NodeMetadata {
                                kind: "func".to_string(),
                                role: None,
                                intent: Some(func.intent.clone()),
                                purpose: None,
                                fields: HashMap::new(),
                                input: func.input.clone(),
                                output: func.output.clone(),
                            },
                        });

                        // Add func -> schema links
                        for input_schema in &func.input {
                            links.push(Link {
                                source: input_schema.clone(),
                                target: format!("func.{}", func.name),
                                label: "input".to_string(),
                                link_type: "func_input".to_string(),
                            });
                        }
                        for output_schema in &func.output {
                            links.push(Link {
                                source: format!("func.{}", func.name),
                                target: output_schema.clone(),
                                label: "output".to_string(),
                                link_type: "func_output".to_string(),
                            });
                        }
                    }
                    Section::Mod(module) => {
                        nodes.push(Node {
                            id: format!("mod.{}", module.name),
                            label: module.name.clone(),
                            node_type: "mod".to_string(),
                            group: "module".to_string(),
                            metadata: NodeMetadata {
                                kind: "mod".to_string(),
                                role: None,
                                intent: None,
                                purpose: Some(module.purpose.clone()),
                                fields: HashMap::new(),
                                input: Vec::new(),
                                output: Vec::new(),
                            },
                        });

                        // Add mod -> schema links
                        for schema_ref in &module.schemas {
                            links.push(Link {
                                source: format!("mod.{}", module.name),
                                target: schema_ref.clone(),
                                label: "uses".to_string(),
                                link_type: "mod_schema".to_string(),
                            });
                        }

                        // Add mod -> func links
                        for func_ref in &module.funcs {
                            links.push(Link {
                                source: format!("mod.{}", module.name),
                                target: func_ref.clone(),
                                label: "uses".to_string(),
                                link_type: "mod_func".to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // Add module dependencies (requires)
        let requires = project.collect_normalized_requires();
        for req in requires {
            links.push(Link {
                source: req.from_mod.clone(),
                target: req.to_mod.clone(),
                label: "requires".to_string(),
                link_type: "mod_require".to_string(),
            });
        }

        GraphData { nodes, links }
    }

    fn generate_html(&self, graph_json: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Surv IR Interactive Visualization</title>
    <script src="https://d3js.org/d3.v7.min.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #1a1a1a;
            color: #e0e0e0;
            overflow: hidden;
        }}

        #container {{
            display: flex;
            height: 100vh;
        }}

        #sidebar {{
            width: 300px;
            background: #252525;
            border-right: 1px solid #444;
            overflow-y: auto;
            padding: 20px;
        }}

        #graph {{
            flex: 1;
            position: relative;
        }}

        h1 {{
            font-size: 18px;
            margin-bottom: 20px;
            color: #fff;
        }}

        .search-box {{
            width: 100%;
            padding: 8px 12px;
            background: #333;
            border: 1px solid #555;
            border-radius: 4px;
            color: #e0e0e0;
            margin-bottom: 15px;
        }}

        .search-box:focus {{
            outline: none;
            border-color: #4a9eff;
        }}

        .filter-group {{
            margin-bottom: 20px;
        }}

        .filter-group h3 {{
            font-size: 12px;
            text-transform: uppercase;
            color: #999;
            margin-bottom: 10px;
        }}

        .filter-option {{
            display: flex;
            align-items: center;
            padding: 6px 0;
            cursor: pointer;
        }}

        .filter-option input {{
            margin-right: 8px;
        }}

        .filter-option label {{
            cursor: pointer;
            font-size: 14px;
        }}

        .color-indicator {{
            display: inline-block;
            width: 12px;
            height: 12px;
            border-radius: 50%;
            margin-right: 6px;
        }}

        .controls {{
            position: absolute;
            top: 20px;
            right: 20px;
            display: flex;
            gap: 10px;
            z-index: 100;
        }}

        .btn {{
            padding: 8px 16px;
            background: #333;
            border: 1px solid #555;
            border-radius: 4px;
            color: #e0e0e0;
            cursor: pointer;
            font-size: 14px;
            transition: background 0.2s;
        }}

        .btn:hover {{
            background: #444;
        }}

        .detail-panel {{
            position: absolute;
            top: 80px;
            right: 20px;
            width: 350px;
            max-height: 80vh;
            background: #252525;
            border: 1px solid #444;
            border-radius: 8px;
            padding: 20px;
            overflow-y: auto;
            display: none;
            z-index: 100;
        }}

        .detail-panel.active {{
            display: block;
        }}

        .detail-panel h2 {{
            font-size: 18px;
            margin-bottom: 10px;
            color: #4a9eff;
        }}

        .detail-panel .close-btn {{
            position: absolute;
            top: 15px;
            right: 15px;
            background: none;
            border: none;
            color: #999;
            font-size: 24px;
            cursor: pointer;
        }}

        .detail-section {{
            margin-bottom: 15px;
        }}

        .detail-section h4 {{
            font-size: 12px;
            text-transform: uppercase;
            color: #999;
            margin-bottom: 6px;
        }}

        .detail-section p {{
            font-size: 14px;
            line-height: 1.5;
        }}

        .detail-list {{
            list-style: none;
            font-size: 14px;
        }}

        .detail-list li {{
            padding: 4px 0;
            color: #4a9eff;
        }}

        .field-table {{
            width: 100%;
            font-size: 13px;
            border-collapse: collapse;
        }}

        .field-table td {{
            padding: 4px 8px;
            border-bottom: 1px solid #333;
        }}

        .field-table td:first-child {{
            color: #999;
            width: 40%;
        }}

        svg {{
            width: 100%;
            height: 100%;
        }}

        .node circle {{
            cursor: pointer;
            stroke: #fff;
            stroke-width: 2px;
        }}

        .node text {{
            font-size: 12px;
            pointer-events: none;
            text-anchor: middle;
            fill: #e0e0e0;
        }}

        .link {{
            stroke: #666;
            stroke-opacity: 0.6;
            stroke-width: 1.5px;
            fill: none;
        }}

        .link.highlighted {{
            stroke: #4a9eff;
            stroke-width: 3px;
            stroke-opacity: 1;
        }}

        .node.highlighted circle {{
            stroke: #4a9eff;
            stroke-width: 4px;
        }}

        .node.dimmed {{
            opacity: 0.2;
        }}

        .link.dimmed {{
            opacity: 0.1;
        }}

        .link-label {{
            font-size: 10px;
            fill: #999;
            pointer-events: none;
        }}
    </style>
</head>
<body>
    <div id="container">
        <div id="sidebar">
            <h1>Surv IR Visualization</h1>

            <input type="text" class="search-box" id="search" placeholder="Search nodes...">

            <div class="filter-group">
                <h3>Node Types</h3>
                <div class="filter-option">
                    <input type="checkbox" id="filter-schema" checked>
                    <label for="filter-schema">
                        <span class="color-indicator" style="background: #2980b9;"></span>
                        Schemas
                    </label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="filter-func" checked>
                    <label for="filter-func">
                        <span class="color-indicator" style="background: #27ae60;"></span>
                        Functions
                    </label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="filter-mod" checked>
                    <label for="filter-mod">
                        <span class="color-indicator" style="background: #8e44ad;"></span>
                        Modules
                    </label>
                </div>
            </div>

            <div class="filter-group">
                <h3>Schema Types</h3>
                <div class="filter-option">
                    <input type="checkbox" id="filter-node" checked>
                    <label for="filter-node">Node</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="filter-edge" checked>
                    <label for="filter-edge">Edge</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="filter-boundary" checked>
                    <label for="filter-boundary">Boundary</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="filter-space" checked>
                    <label for="filter-space">Space</label>
                </div>
            </div>

            <div class="filter-group">
                <h3>Link Types</h3>
                <div class="filter-option">
                    <input type="checkbox" id="link-func" checked>
                    <label for="link-func">Function I/O</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="link-mod" checked>
                    <label for="link-mod">Module Uses</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="link-require" checked>
                    <label for="link-require">Module Requires</label>
                </div>
                <div class="filter-option">
                    <input type="checkbox" id="link-schema" checked>
                    <label for="link-schema">Schema Relations</label>
                </div>
            </div>
        </div>

        <div id="graph">
            <div class="controls">
                <button class="btn" id="reset-zoom">Reset View</button>
                <button class="btn" id="reset-positions">Reset Layout</button>
            </div>

            <div class="detail-panel" id="detail-panel">
                <button class="close-btn" id="close-detail">&times;</button>
                <div id="detail-content"></div>
            </div>

            <svg id="svg"></svg>
        </div>
    </div>

    <script>
        const data = {graph_json};

        const width = document.getElementById('graph').clientWidth;
        const height = document.getElementById('graph').clientHeight;

        const svg = d3.select('#svg');
        const g = svg.append('g');

        const zoom = d3.zoom()
            .scaleExtent([0.1, 4])
            .on('zoom', (event) => {{
                g.attr('transform', event.transform);
            }});

        svg.call(zoom);

        const colorMap = {{
            'node': '#2980b9',
            'edge': '#27ae60',
            'boundary': '#f39c12',
            'space': '#8e44ad',
            'function': '#27ae60',
            'module': '#8e44ad'
        }};

        const simulation = d3.forceSimulation(data.nodes)
            .force('link', d3.forceLink(data.links).id(d => d.id).distance(150))
            .force('charge', d3.forceManyBody().strength(-400))
            .force('center', d3.forceCenter(width / 2, height / 2))
            .force('collision', d3.forceCollide().radius(40));

        const link = g.append('g')
            .selectAll('path')
            .data(data.links)
            .join('path')
            .attr('class', 'link')
            .attr('data-type', d => d.type)
            .attr('marker-end', 'url(#arrow)');

        svg.append('defs').append('marker')
            .attr('id', 'arrow')
            .attr('viewBox', '0 -5 10 10')
            .attr('refX', 25)
            .attr('refY', 0)
            .attr('markerWidth', 6)
            .attr('markerHeight', 6)
            .attr('orient', 'auto')
            .append('path')
            .attr('d', 'M0,-5L10,0L0,5')
            .attr('fill', '#666');

        const node = g.append('g')
            .selectAll('g')
            .data(data.nodes)
            .join('g')
            .attr('class', 'node')
            .attr('data-type', d => d.type)
            .attr('data-group', d => d.group)
            .call(d3.drag()
                .on('start', dragstarted)
                .on('drag', dragged)
                .on('end', dragended));

        node.append('circle')
            .attr('r', 20)
            .attr('fill', d => colorMap[d.group] || '#999');

        node.append('text')
            .attr('dy', 35)
            .text(d => d.label);

        node.on('click', (event, d) => {{
            event.stopPropagation();
            showDetail(d);
            highlightConnections(d);
        }});

        svg.on('click', () => {{
            hideDetail();
            clearHighlight();
        }});

        simulation.on('tick', () => {{
            link.attr('d', d => {{
                const dx = d.target.x - d.source.x;
                const dy = d.target.y - d.source.y;
                return `M${{d.source.x}},${{d.source.y}}L${{d.target.x}},${{d.target.y}}`;
            }});

            node.attr('transform', d => `translate(${{d.x}},${{d.y}})`);
        }});

        function dragstarted(event, d) {{
            if (!event.active) simulation.alphaTarget(0.3).restart();
            d.fx = d.x;
            d.fy = d.y;
        }}

        function dragged(event, d) {{
            d.fx = event.x;
            d.fy = event.y;
        }}

        function dragended(event, d) {{
            if (!event.active) simulation.alphaTarget(0);
            d.fx = null;
            d.fy = null;
        }}

        function showDetail(d) {{
            const panel = document.getElementById('detail-panel');
            const content = document.getElementById('detail-content');

            let html = `<h2>${{d.label}}</h2>`;
            html += `<div class="detail-section"><h4>Type</h4><p>${{d.type}} (${{d.group}})</p></div>`;

            if (d.metadata.kind) {{
                html += `<div class="detail-section"><h4>Kind</h4><p>${{d.metadata.kind}}</p></div>`;
            }}

            if (d.metadata.role) {{
                html += `<div class="detail-section"><h4>Role</h4><p>${{d.metadata.role}}</p></div>`;
            }}

            if (d.metadata.intent) {{
                html += `<div class="detail-section"><h4>Intent</h4><p>${{d.metadata.intent}}</p></div>`;
            }}

            if (d.metadata.purpose) {{
                html += `<div class="detail-section"><h4>Purpose</h4><p>${{d.metadata.purpose}}</p></div>`;
            }}

            if (d.metadata.input && d.metadata.input.length > 0) {{
                html += `<div class="detail-section"><h4>Input</h4><ul class="detail-list">`;
                d.metadata.input.forEach(i => html += `<li>${{i}}</li>`);
                html += `</ul></div>`;
            }}

            if (d.metadata.output && d.metadata.output.length > 0) {{
                html += `<div class="detail-section"><h4>Output</h4><ul class="detail-list">`;
                d.metadata.output.forEach(o => html += `<li>${{o}}</li>`);
                html += `</ul></div>`;
            }}

            if (Object.keys(d.metadata.fields).length > 0) {{
                html += `<div class="detail-section"><h4>Fields</h4><table class="field-table">`;
                for (const [key, value] of Object.entries(d.metadata.fields)) {{
                    html += `<tr><td>${{key}}</td><td>${{value}}</td></tr>`;
                }}
                html += `</table></div>`;
            }}

            content.innerHTML = html;
            panel.classList.add('active');
        }}

        function hideDetail() {{
            document.getElementById('detail-panel').classList.remove('active');
        }}

        function highlightConnections(d) {{
            const connectedNodes = new Set([d.id]);
            const connectedLinks = new Set();

            data.links.forEach(l => {{
                if (l.source.id === d.id || l.target.id === d.id) {{
                    connectedLinks.add(l);
                    connectedNodes.add(l.source.id);
                    connectedNodes.add(l.target.id);
                }}
            }});

            node.classed('dimmed', n => !connectedNodes.has(n.id))
                .classed('highlighted', n => n.id === d.id);

            link.classed('dimmed', l => !connectedLinks.has(l))
                .classed('highlighted', l => connectedLinks.has(l));
        }}

        function clearHighlight() {{
            node.classed('dimmed', false).classed('highlighted', false);
            link.classed('dimmed', false).classed('highlighted', false);
        }}

        // Search
        document.getElementById('search').addEventListener('input', (e) => {{
            const query = e.target.value.toLowerCase();
            node.classed('dimmed', d => !d.label.toLowerCase().includes(query));
        }});

        // Filters
        function updateFilters() {{
            const typeFilters = {{
                'schema': document.getElementById('filter-schema').checked,
                'func': document.getElementById('filter-func').checked,
                'mod': document.getElementById('filter-mod').checked
            }};

            const groupFilters = {{
                'node': document.getElementById('filter-node').checked,
                'edge': document.getElementById('filter-edge').checked,
                'boundary': document.getElementById('filter-boundary').checked,
                'space': document.getElementById('filter-space').checked,
                'function': true,
                'module': true
            }};

            const linkFilters = {{
                'func_input': document.getElementById('link-func').checked,
                'func_output': document.getElementById('link-func').checked,
                'mod_schema': document.getElementById('link-mod').checked,
                'mod_func': document.getElementById('link-mod').checked,
                'mod_require': document.getElementById('link-require').checked,
                'edge_from': document.getElementById('link-schema').checked,
                'edge_to': document.getElementById('link-schema').checked,
                'boundary': document.getElementById('link-schema').checked,
                'space': document.getElementById('link-schema').checked
            }};

            node.style('display', d => {{
                return typeFilters[d.type] && groupFilters[d.group] ? 'block' : 'none';
            }});

            link.style('display', l => {{
                return linkFilters[l.type] ? 'block' : 'none';
            }});
        }}

        document.querySelectorAll('input[type="checkbox"]').forEach(cb => {{
            cb.addEventListener('change', updateFilters);
        }});

        // Controls
        document.getElementById('reset-zoom').addEventListener('click', () => {{
            svg.transition().duration(750).call(
                zoom.transform,
                d3.zoomIdentity.translate(0, 0).scale(1)
            );
        }});

        document.getElementById('reset-positions').addEventListener('click', () => {{
            simulation.alpha(1).restart();
        }});

        document.getElementById('close-detail').addEventListener('click', () => {{
            hideDetail();
            clearHighlight();
        }});
    </script>
</body>
</html>"#,
            graph_json = graph_json
        )
    }

    fn generate_deploy_html(&self, graph_json: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Deploy Pipeline Visualization</title>
    <script src="https://d3js.org/d3.v7.min.js"></script>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
            background: #1a1a2e;
            color: #eee;
            overflow: hidden;
        }}

        #graph {{
            width: 100vw;
            height: 100vh;
        }}

        .node {{
            cursor: pointer;
            stroke: #fff;
            stroke-width: 2px;
        }}

        .node.production {{
            fill: #ff6b6b;
        }}

        .node.staging {{
            fill: #ffd43b;
        }}

        .node.default {{
            fill: #4ecdc4;
        }}

        .node:hover {{
            stroke-width: 3px;
            filter: brightness(1.2);
        }}

        .node.selected {{
            stroke: #fff;
            stroke-width: 4px;
        }}

        .link {{
            stroke: #999;
            stroke-width: 2px;
            fill: none;
            marker-end: url(#arrowhead);
        }}

        .link.highlighted {{
            stroke: #4ecdc4;
            stroke-width: 3px;
        }}

        .node-label {{
            fill: #fff;
            font-size: 12px;
            font-weight: 600;
            text-anchor: middle;
            pointer-events: none;
            text-shadow: 1px 1px 2px rgba(0,0,0,0.8);
        }}

        #details-panel {{
            position: fixed;
            top: 20px;
            right: 20px;
            width: 350px;
            max-height: calc(100vh - 40px);
            background: rgba(22, 22, 34, 0.95);
            border: 1px solid #4ecdc4;
            border-radius: 8px;
            padding: 20px;
            display: none;
            overflow-y: auto;
            box-shadow: 0 8px 32px rgba(0,0,0,0.4);
        }}

        #details-panel h2 {{
            color: #4ecdc4;
            margin-bottom: 15px;
            font-size: 18px;
            border-bottom: 2px solid #4ecdc4;
            padding-bottom: 8px;
        }}

        #details-panel .section {{
            margin: 15px 0;
        }}

        #details-panel .section h3 {{
            color: #ffd43b;
            font-size: 14px;
            margin-bottom: 8px;
        }}

        #details-panel .item {{
            background: rgba(78, 205, 196, 0.1);
            padding: 8px;
            margin: 5px 0;
            border-radius: 4px;
            font-size: 13px;
            border-left: 3px solid #4ecdc4;
        }}

        #details-panel .warning {{
            background: rgba(255, 107, 107, 0.1);
            border-left-color: #ff6b6b;
        }}

        #details-panel .badge {{
            display: inline-block;
            background: #4ecdc4;
            color: #1a1a2e;
            padding: 2px 8px;
            border-radius: 12px;
            font-size: 11px;
            font-weight: 600;
            margin-right: 5px;
        }}

        #details-panel .badge.prod {{
            background: #ff6b6b;
            color: #fff;
        }}

        #details-panel .badge.staging {{
            background: #ffd43b;
            color: #1a1a2e;
        }}

        #controls {{
            position: fixed;
            top: 20px;
            left: 20px;
            background: rgba(22, 22, 34, 0.95);
            border: 1px solid #4ecdc4;
            border-radius: 8px;
            padding: 15px;
            box-shadow: 0 4px 16px rgba(0,0,0,0.3);
        }}

        #controls h3 {{
            color: #4ecdc4;
            font-size: 14px;
            margin-bottom: 10px;
        }}

        #search {{
            width: 200px;
            padding: 8px;
            background: #16161e;
            border: 1px solid #4ecdc4;
            border-radius: 4px;
            color: #eee;
            font-size: 13px;
        }}

        #legend {{
            position: fixed;
            bottom: 20px;
            left: 20px;
            background: rgba(22, 22, 34, 0.95);
            border: 1px solid #4ecdc4;
            border-radius: 8px;
            padding: 15px;
            box-shadow: 0 4px 16px rgba(0,0,0,0.3);
        }}

        #legend h3 {{
            color: #4ecdc4;
            font-size: 14px;
            margin-bottom: 10px;
        }}

        .legend-item {{
            display: flex;
            align-items: center;
            margin: 8px 0;
            font-size: 13px;
        }}

        .legend-color {{
            width: 20px;
            height: 20px;
            border-radius: 3px;
            margin-right: 10px;
            border: 1px solid #fff;
        }}

        .close-btn {{
            position: absolute;
            top: 15px;
            right: 15px;
            background: none;
            border: none;
            color: #ff6b6b;
            font-size: 20px;
            cursor: pointer;
            padding: 5px;
        }}

        code {{
            background: rgba(78, 205, 196, 0.1);
            padding: 2px 6px;
            border-radius: 3px;
            font-family: 'Courier New', monospace;
            font-size: 12px;
        }}
    </style>
</head>
<body>
    <svg id="graph"></svg>

    <div id="controls">
        <h3>🔍 Search Jobs</h3>
        <input type="text" id="search" placeholder="Search job names...">
    </div>

    <div id="legend">
        <h3>📊 Target Types</h3>
        <div class="legend-item">
            <div class="legend-color" style="background: #ff6b6b;"></div>
            <span>Production</span>
        </div>
        <div class="legend-item">
            <div class="legend-color" style="background: #ffd43b;"></div>
            <span>Staging</span>
        </div>
        <div class="legend-item">
            <div class="legend-color" style="background: #4ecdc4;"></div>
            <span>Build/Test</span>
        </div>
    </div>

    <div id="details-panel">
        <button class="close-btn" onclick="closeDetails()">×</button>
        <div id="details-content"></div>
    </div>

    <script>
        const graphData = {graph_json};

        const width = window.innerWidth;
        const height = window.innerHeight;

        const svg = d3.select('#graph')
            .attr('width', width)
            .attr('height', height);

        // Define arrowhead marker
        svg.append('defs').append('marker')
            .attr('id', 'arrowhead')
            .attr('viewBox', '0 -5 10 10')
            .attr('refX', 25)
            .attr('refY', 0)
            .attr('markerWidth', 6)
            .attr('markerHeight', 6)
            .attr('orient', 'auto')
            .append('path')
            .attr('d', 'M0,-5L10,0L0,5')
            .attr('fill', '#999');

        const g = svg.append('g');

        // Zoom behavior
        const zoom = d3.zoom()
            .scaleExtent([0.1, 4])
            .on('zoom', (event) => {{
                g.attr('transform', event.transform);
            }});

        svg.call(zoom);

        // Create force simulation
        const simulation = d3.forceSimulation(graphData.nodes)
            .force('link', d3.forceLink(graphData.links)
                .id(d => d.id)
                .distance(150))
            .force('charge', d3.forceManyBody().strength(-500))
            .force('center', d3.forceCenter(width / 2, height / 2))
            .force('collision', d3.forceCollide().radius(50));

        // Draw links
        const link = g.append('g')
            .selectAll('path')
            .data(graphData.links)
            .join('path')
            .attr('class', 'link');

        // Draw nodes
        const node = g.append('g')
            .selectAll('circle')
            .data(graphData.nodes)
            .join('circle')
            .attr('class', d => `node ${{d.group}}`)
            .attr('r', 20)
            .call(drag(simulation))
            .on('click', showDetails);

        // Draw labels
        const label = g.append('g')
            .selectAll('text')
            .data(graphData.nodes)
            .join('text')
            .attr('class', 'node-label')
            .attr('dy', -25)
            .text(d => d.label);

        // Update positions on tick
        simulation.on('tick', () => {{
            link.attr('d', d => {{
                const dx = d.target.x - d.source.x;
                const dy = d.target.y - d.source.y;
                return `M${{d.source.x}},${{d.source.y}} L${{d.target.x}},${{d.target.y}}`;
            }});

            node
                .attr('cx', d => d.x)
                .attr('cy', d => d.y);

            label
                .attr('x', d => d.x)
                .attr('y', d => d.y);
        }});

        // Drag behavior
        function drag(simulation) {{
            function dragstarted(event) {{
                if (!event.active) simulation.alphaTarget(0.3).restart();
                event.subject.fx = event.subject.x;
                event.subject.fy = event.subject.y;
            }}

            function dragged(event) {{
                event.subject.fx = event.x;
                event.subject.fy = event.y;
            }}

            function dragended(event) {{
                if (!event.active) simulation.alphaTarget(0);
                event.subject.fx = null;
                event.subject.fy = null;
            }}

            return d3.drag()
                .on('start', dragstarted)
                .on('drag', dragged)
                .on('end', dragended);
        }}

        // Show details panel
        function showDetails(event, d) {{
            const panel = document.getElementById('details-panel');
            const content = document.getElementById('details-content');

            // Clear previous selection
            node.classed('selected', false);
            d3.select(this).classed('selected', true);

            let html = `<h2>${{d.label}}</h2>`;

            if (d.metadata.target_kind) {{
                const badgeClass = d.metadata.target_kind === 'production' ? 'prod' :
                                 d.metadata.target_kind === 'staging' ? 'staging' : '';
                html += `<span class="badge ${{badgeClass}}">${{d.metadata.target_kind}}</span>`;
            }}

            if (d.metadata.target) {{
                html += `<div class="section">
                    <h3>🎯 Target</h3>
                    <div class="item"><code>${{d.metadata.target}}</code></div>
                </div>`;
            }}

            if (d.metadata.commands && d.metadata.commands.length > 0) {{
                html += `<div class="section"><h3>⚙️ Commands</h3>`;
                d.metadata.commands.forEach(cmd => {{
                    html += `<div class="item"><code>${{cmd}}</code></div>`;
                }});
                html += `</div>`;
            }}

            if (d.metadata.secrets && d.metadata.secrets.length > 0) {{
                html += `<div class="section"><h3>🔐 Secrets</h3>`;
                d.metadata.secrets.forEach(s => {{
                    html += `<div class="item">${{s}}</div>`;
                }});
                html += `</div>`;
            }}

            if (d.metadata.artifacts && d.metadata.artifacts.length > 0) {{
                html += `<div class="section"><h3>📦 Produces</h3>`;
                d.metadata.artifacts.forEach(a => {{
                    html += `<div class="item">${{a}}</div>`;
                }});
                html += `</div>`;
            }}

            if (d.metadata.side_effects && d.metadata.side_effects.length > 0) {{
                html += `<div class="section"><h3>⚠️ Side Effects</h3>`;
                d.metadata.side_effects.forEach(se => {{
                    html += `<div class="item warning">${{se}}</div>`;
                }});
                html += `</div>`;
            }}

            if (d.metadata.permissions) {{
                html += `<div class="section">
                    <h3>🔑 Permissions</h3>
                    <div class="item">${{d.metadata.permissions}}</div>
                </div>`;
            }}

            // Show dependencies
            const deps = graphData.links.filter(l => l.target.id === d.id);
            if (deps.length > 0) {{
                html += `<div class="section"><h3>⬅️ Depends On</h3>`;
                deps.forEach(dep => {{
                    const source = graphData.nodes.find(n => n.id === dep.source.id);
                    html += `<div class="item">${{source.label}}</div>`;
                }});
                html += `</div>`;
            }}

            const dependents = graphData.links.filter(l => l.source.id === d.id);
            if (dependents.length > 0) {{
                html += `<div class="section"><h3>➡️ Required By</h3>`;
                dependents.forEach(dep => {{
                    const target = graphData.nodes.find(n => n.id === dep.target.id);
                    html += `<div class="item">${{target.label}}</div>`;
                }});
                html += `</div>`;
            }}

            content.innerHTML = html;
            panel.style.display = 'block';

            // Highlight related nodes
            link.classed('highlighted', l => l.source.id === d.id || l.target.id === d.id);
        }}

        function closeDetails() {{
            document.getElementById('details-panel').style.display = 'none';
            node.classed('selected', false);
            link.classed('highlighted', false);
        }}

        // Search functionality
        document.getElementById('search').addEventListener('input', (e) => {{
            const query = e.target.value.toLowerCase();
            node.style('opacity', d => {{
                if (!query) return 1;
                return d.label.toLowerCase().includes(query) ? 1 : 0.2;
            }});
            label.style('opacity', d => {{
                if (!query) return 1;
                return d.label.toLowerCase().includes(query) ? 1 : 0.2;
            }});
        }});

        // Click outside to close
        svg.on('click', (event) => {{
            if (event.target === event.currentTarget) {{
                closeDetails();
            }}
        }});
    </script>
</body>
</html>"#,
            graph_json = graph_json
        )
    }
}

impl Default for HtmlExporter {
    fn default() -> Self {
        Self::new()
    }
}

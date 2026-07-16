use crate::core::swagger::spec::generate_openapi_spec;

pub fn render_swagger_ui() -> maud::Markup {
    let spec_json = serde_json::to_string_pretty(&generate_openapi_spec()).unwrap();

    maud::html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "OpenApi API Documentation - Swagger UI" }
                link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui.css";
                link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5.17.14/favicon-32x32.png" sizes="32x32";
                link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5.17.14/favicon-16x16.png" sizes="16x16";
                style {
                "
                html { box-sizing: border-box; overflow-y: scroll; }
                *, *:before, *:after { box-sizing: inherit; }
                body { margin: 0; background: #0b0b10; font-family: sans-serif; }
                
                /* Main container */
                #swagger-ui { 
                    background: #0b0b10; 
                    min-height: 100vh; 
                    max-width: 80%; 
                    margin: auto; 
                }
                
                /* Info section */
                .swagger-ui .info .title p { color: #ffffff; }
                .swagger-ui .info .title small { color: #64ffda; }
                .swagger-ui .info .description { color: #ffffff; }
                .swagger-ui .info .title { color: #ffffff; }
                .swagger-ui .info a { color: #64ffda; }
                
                /* Scheme container */
                .swagger-ui .scheme-container { 
                    background: #16213e; 
                    border-bottom: 1px solid #2a2a3e;
                }
                
                /* Operation blocks */
                .swagger-ui .opblock { 
                    background: #1a1a2e; 
                    border-color: #2a2a3e; 
                }
                .swagger-ui .opblock .opblock-summary-method { min-width: 80px; }
                .swagger-ui .opblock .opblock-summary-path { color: #ffffff !important; }
                .swagger-ui .opblock .opblock-summary-path a { color: #ffffff !important; }
                .swagger-ui .opblock .opblock-summary-path span { color: #ffffff !important; }
                .swagger-ui .opblock .opblock-summary-description { color: #ffffff !important; }
                .swagger-ui .opblock-description-wrapper p { color: #ffffff; }
                
                /* 👇 FIX: Operation section header (Parameters, Request body, Responses) */
                .swagger-ui .opblock-section-header { 
                    background: #1a1a2e !important; 
                    border-bottom: 1px solid #2a2a3e !important; 
                    padding: 10px 20px !important;
                }
                .swagger-ui .opblock-section-header h4 { 
                    color: #ffffff !important; 
                }
                .swagger-ui .opblock-section-header .btn-group {
                    background: transparent !important;
                }
                .swagger-ui .opblock .opblock-section .parameters-container .parameters-no-params {
                    color: #ffffff !important;
                    background: transparent !important;
                }
                
                /* Parameters section */
                .swagger-ui .parameters-col_description { color: #ffffff; }
                .swagger-ui .parameters-col_description .markdown p { color: #ffffff; }
                .swagger-ui .parameter__name { color: #ffffff !important; }
                .swagger-ui .parameter__type { color: #64ffda !important; }
                .swagger-ui .parameter__in { color: #cccccc !important; }
                
                /* Request body */
                .swagger-ui .opblock-section .opblock-section-header .btn-group .btn {
                    color: #64ffda !important;
                    border-color: #64ffda !important;
                }
                
                /* Response section */
                .swagger-ui .responses-wrapper .responses-description { color: #ffffff; }
                .swagger-ui .responses-inner .response-col_status { color: #64ffda !important; }
                .swagger-ui .responses-inner .response-col_description .markdown p { color: #ffffff; }
                
                /* Input fields */
                .swagger-ui .body-param__text { 
                    color: #ffffff !important; 
                    background: #0d0d1a !important;
                    border-color: #2a2a3e !important;
                }
                .swagger-ui .body-param__text:focus { 
                    border-color: #64ffda !important; 
                    outline: none;
                }
                .swagger-ui textarea { 
                    color: #ffffff !important; 
                    background: #0d0d1a !important;
                    border-color: #2a2a3e !important;
                }
                .swagger-ui textarea:focus { 
                    border-color: #64ffda !important; 
                }
                .swagger-ui input[type=text] { 
                    color: #ffffff !important; 
                    background: #0d0d1a !important;
                    border-color: #2a2a3e !important;
                }
                .swagger-ui input[type=text]:focus { 
                    border-color: #64ffda !important; 
                }
                input[type="text"] {
                    color: #ffffff !important;
                    background: #0d0d1a !important;
                }
                select {
                    color: #ffffff !important;
                    background: #0d0d1a !important;
                }
                
                /* Table cells */
                .swagger-ui .parameters-container .parameters table tbody tr td { 
                    color: #ffffff !important; 
                    background: #1a1a2e !important;
                }
                .swagger-ui .parameters-container .parameters table thead tr th { 
                    color: #64ffda !important; 
                    background: #16213e !important;
                }
                .swagger-ui table tbody tr td { 
                    color: #ffffff !important; 
                    background: #1a1a2e !important;
                }
                .swagger-ui table thead tr th { 
                    color: #64ffda !important; 
                    background: #16213e !important;
                }
                
                /* Model examples */
                .swagger-ui .model .property { 
                    color: #ffffff !important; 
                }
                .swagger-ui .model .property .string { 
                    color: #64ffda !important; 
                }
                .swagger-ui .model .property .number { 
                    color: #f59e0b !important; 
                }
                .swagger-ui .model .property .boolean { 
                    color: #f472b6 !important; 
                }
                
                /* Buttons */
                .swagger-ui .btn { 
                    border-color: #64ffda; 
                    color: #64ffda; 
                    background: transparent;
                }
                .swagger-ui .btn:hover { 
                    background: rgba(100, 255, 218, 0.1); 
                }
                .swagger-ui .btn.execute { 
                    background: #1a3a2e; 
                    color: #64ffda;
                    border-color: #64ffda;
                }
                .swagger-ui .btn.execute:hover { 
                    background: #2a4a3e; 
                }
                
                /* Tabs */
                .swagger-ui .tab li { 
                    color: #ffffff !important; 
                }
                .swagger-ui .tab li.selected { 
                    border-bottom-color: #64ffda !important; 
                    color: #64ffda !important; 
                }
                
                /* Models */
                .swagger-ui .model-title { 
                    color: #ffffff !important; 
                }
                .swagger-ui .models { 
                    background: #1a1a2e !important; 
                }
                .swagger-ui .models .model-container { 
                    background: #1a1a2e !important; 
                    border-color: #2a2a3e !important;
                }
                .swagger-ui .models .model-container .model-box { 
                    background: #0d0d1a !important; 
                }
                
                /* Code / JSON preview */
                .swagger-ui .highlight-code pre { 
                    background: #0d0d1a !important; 
                    color: #ffffff !important; 
                    border-color: #2a2a3e !important;
                }
                .swagger-ui .highlight-code pre .json { 
                    color: #ffffff !important; 
                }
                .swagger-ui .json-schema-2020-12 .json-schema-2020-12__title { 
                    color: #ffffff !important; 
                }
                .swagger-ui .json-schema-2020-12 .json-schema-2020-12__property { 
                    color: #ffffff !important; 
                }
                
                /* Headers */
                .swagger-ui .opblock-tag { 
                    color: #ffffff !important; 
                    border-bottom-color: #2a2a3e !important;
                }
                .swagger-ui .opblock-tag:hover { 
                    background: rgba(255, 255, 255, 0.02); 
                }
                
                /* Misc */
                .swagger-ui .loading-container .loading { 
                    color: #ffffff !important; 
                }
                .swagger-ui .dialog-ux .modal-ux { 
                    background: #1a1a2e !important; 
                    border-color: #2a2a3e !important;
                }
                .swagger-ui .dialog-ux .modal-ux .modal-ux-title { 
                    color: #ffffff !important; 
                }
                .swagger-ui .dialog-ux .modal-ux .modal-ux-content p { 
                    color: #ffffff !important; 
                }
                .swagger-ui .dialog-ux .modal-ux .modal-ux-footer .btn { 
                    color: #ffffff !important; 
                }
                
                /* Dropdown */
                .swagger-ui select { 
                    color: #ffffff !important; 
                    background: #0d0d1a !important;
                    border-color: #2a2a3e !important;
                }
                .swagger-ui select:focus { 
                    border-color: #64ffda !important; 
                    outline: none;
                }
                .swagger-ui select option { 
                    background: #0d0d1a !important; 
                    color: #ffffff !important;
                }
                
                /* Scrollbar for dark theme */
                .swagger-ui .renderedMarkdown pre { 
                    background: #0d0d1a !important; 
                    color: #ffffff !important; 
                }
                
                /* Additional override for section headers */
                .swagger-ui .opblock .opblock-section .opblock-section-header {
                    background: #1a1a2e !important;
                    border-bottom: 1px solid #2a2a3e !important;
                }
                .swagger-ui .opblock .opblock-section .opblock-section-header .btn-group .btn-cancel {
                    color: #ffffff !important;
                    background: transparent !important;
                }
                "
            }  
            }
            body {
                div id="swagger-ui" {}
                script src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-bundle.js" {}
                script src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-standalone-preset.js" {}
                script {
                    (maud::PreEscaped(format!(r#"
                        window.onload = function() {{
                            const ui = SwaggerUIBundle({{
                                dom_id: '#swagger-ui',
                                spec: {},
                                presets: [
                                    SwaggerUIBundle.presets.apis,
                                    SwaggerUIStandalonePreset
                                ],
                                layout: "BaseLayout",
                                deepLinking: true,
                                showExtensions: true,
                                showCommonExtensions: true,
                                docExpansion: "list",
                                filter: true,
                                persistAuthorization: true,
                                defaultModelsExpandDepth: 1,
                                defaultModelExpandDepth: 1,
                                tryItOutEnabled: true,
                            }});
                            window.ui = ui;
                        }};
                    "#, spec_json)))
                }
            }
        }
    }
}
import "@typespec/json-schema";
using TypeSpec.JsonSchema;

// Web/admin-web filesystem and generated-source contract.
// Peer authority: ../json-schema/server-layout.schema.json.
// Neither authority is generated from the other.
namespace Ores.ServerLayout.V1;

@id("WebSurface")
enum WebSurface {
  web: "web",
  admin_web: "admin_web",
}

@id("WebFilesystemContract")
model WebFilesystemContract {
  root: "src/pages";
  page_leaf: "page.rs";
  generator_leaf: "gen.rs";
  lambda_leaf: "lambda.rs";
  layout_leaf: "layout.rs";
  url_mapping: "one_to_one";
  route_groups: false;
  dynamic_segment: "[param]";
  catch_all_segment: "[...param]";
  optional_catch_all_segment: "[[...param]]";
}

@id("LayoutInheritanceContract")
model LayoutInheritanceContract {
  enabled: true;
  leaf: "layout.rs";
  inheritance: "ancestor_chain";
  discovery_order: "outer_to_inner";
  application_order: "inner_to_outer";
  boundary: "src/pages";
  affects_url: false;
  function_name: "layout";
}

@id("WebLambdaSourceContract")
model WebLambdaSourceContract {
  generated: true;
  provider_neutral: true;
  sibling_of: "page.rs";
  generated_by: "ores-stack";
  semantic_authority: "page.rs";
}

@id("WebServerLayoutContract")
model WebServerLayoutContract {
  schema_version: "ores.server-layout.v1";
  surface: WebSurface;
  filesystem: WebFilesystemContract;
  layout: LayoutInheritanceContract;
  lambda: WebLambdaSourceContract;
}

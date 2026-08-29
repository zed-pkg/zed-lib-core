import { defineConfig } from "drizzle-kit";

const databaseUrl = process.env.DDL_ROUNDTRIP_DATABASE_URL;
const outputDirectory = process.env.DDL_ROUNDTRIP_DRIZZLE_OUT;

if (!databaseUrl) {
  throw new Error("DDL_ROUNDTRIP_DATABASE_URL is required");
}

if (!outputDirectory) {
  throw new Error("DDL_ROUNDTRIP_DRIZZLE_OUT is required");
}

export default defineConfig({
  dialect: "postgresql",
  dbCredentials: { url: databaseUrl },
  out: outputDirectory,
  schemaFilter: ["public"],
  introspect: { casing: "preserve" },
});

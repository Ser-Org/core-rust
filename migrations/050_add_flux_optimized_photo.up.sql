-- 050_add_flux_optimized_photo.up.sql
-- Adds a derivative photo reference to user_photos. The derivative is a
-- downscaled (~1 MP) JPEG copy of the original upload, used as the `input_image`
-- for Flux compositing calls so BFL's input-megapixel billing is bounded.
-- Columns are nullable: legacy rows and failed-resize paths fall back to
-- storage_url / storage_path.

ALTER TABLE user_photos
    ADD COLUMN flux_storage_url  TEXT,
    ADD COLUMN flux_storage_path TEXT;

ALTER TABLE memory_recall_wider ALTER COLUMN commit_id TYPE VARCHAR(256);
ALTER TABLE memory_recall_wider DROP CONSTRAINT memory_recall_wider_commit_id_check;
ALTER TABLE memory_recall_wider ADD CONSTRAINT memory_recall_wider_commit_id_check CHECK (length(commit_id) >= 1);

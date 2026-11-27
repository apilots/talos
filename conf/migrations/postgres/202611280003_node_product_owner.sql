ALTER TABLE agent_nodes ADD COLUMN product_owner TEXT CHECK (product_owner IN ('computer', 'server'));

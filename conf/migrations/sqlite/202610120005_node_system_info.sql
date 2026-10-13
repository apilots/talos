ALTER TABLE agent_nodes ADD COLUMN system_info TEXT CHECK(system_info IS NULL OR json_valid(system_info));

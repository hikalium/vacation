default:
	cargo build

.PHONY : default input_test output_test

input_test:
	cargo run -- --input third_party/sample_vrm/VRM1_Constraint_Twist_Sample.vrm

output_test:
	cargo run -- --output generated/test.glb

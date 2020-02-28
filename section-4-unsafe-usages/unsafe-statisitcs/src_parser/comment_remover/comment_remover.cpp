#include <iostream>
#include <fstream>
#include <string>
using namespace std;

enum class ParseState {
	INIT,
	ONESLASH,
	TWOSLASH,
	NOCOMMENT,
	LEFTSTAR,
	RIGHTSTAR
};

int remove_comment(std::string& src_code) {

	auto state = ParseState::INIT;
	std::string nocomment;
	for (auto& ch : src_code) {
		switch (state) {
			case ParseState::INIT: {
				switch (ch) {
					case '/': {
						state = ParseState::ONESLASH;
						break;
						  }
					default: {
						state = ParseState::NOCOMMENT;
						nocomment.push_back(ch);
						break;
						 }
				}
				break;
			}
			case ParseState::ONESLASH: {
				switch (ch) {
					case '/': {
						state = ParseState::TWOSLASH;
						break;
						  }
					case '*': {
						state = ParseState::LEFTSTAR;
						break;
						  }
					default: {
						state = ParseState::NOCOMMENT;
						nocomment.push_back('/');
						nocomment.push_back(ch);
						break;
						 }
				}
				break;
			}
			case ParseState::TWOSLASH: {
				switch (ch) {
					case '\n': {
						state = ParseState::NOCOMMENT;
						nocomment.push_back(ch);
						break;
					}
					default: {
						break;
						 }

				}
				break;
					
			}
			case ParseState::NOCOMMENT: {
				switch (ch) {
					case '/': {
						state = ParseState::ONESLASH;
						break;
					}
					default: {
						state = ParseState::NOCOMMENT;
						nocomment.push_back(ch);
						break;
					}
				}
				break;
			}
			case ParseState::LEFTSTAR: {
				switch (ch) {
					case '*': {
						state = ParseState::RIGHTSTAR;
						break;
					}
					case '\n': {
						nocomment.push_back(ch);
						break;
					}
					default: {
						break;
					}
				}
				break;
			}
			case ParseState::RIGHTSTAR: {
				switch (ch) {
					case '/': {
						state = ParseState::NOCOMMENT;
						break;
					}
					case '\n': {
						nocomment.push_back(ch);
						break;
					}
					default: {
						break;
					}
				}
				break;
			}
			default: {
				cerr << "Unreachable!\n";
				break;

				 }
		}
	}
	cout << nocomment;
	return 0;
}

int main(int argc, char** argv) {
	if (argc < 2 || std::string(argv[1]) == "") {
		cerr << "Wrong input\n";
		return 1;
	}
	
	std::ifstream t(argv[1]);
	std::string content((std::istreambuf_iterator<char>(t)), std::istreambuf_iterator<char>());
	remove_comment(content);
}

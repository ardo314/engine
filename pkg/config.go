package internal

import "encoding/json"

type Config[T any] struct {
	value T
}

func NewConfig[T any]() *Config[T] {
	return &Config[T]{}
}

func (c *Config[T]) load() {
	json.Unmarshal([]byte(`{"name":"test","value":42}`), &c.value)
}

func (c *Config[T]) Get() *T {
	c.load()
	return &c.value
}

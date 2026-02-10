package internal

type Data[T any] struct {
	value T
}

func NewData[T any]() *Data[T] {
	return &Data[T]{}
}

func (d *Data[T]) Get() T {
	return d.value
}

package nova_ext

import (
	"context"
	"fmt"
	"log"
	"net/http"

	v2 "github.com/wandelbotsgmbh/nova-api-client-go/v25/pkg/nova/v2"
)

func printMotionGroupPositions(client *v2.ClientWithResponses, cell, controller string) {
	resp, err := client.GetControllerDescriptionWithResponse(context.TODO(), cell, controller)
	if err != nil {
		log.Printf("Failed to get robot controller %s: %v", controller, err)
		return
	}

	if resp.JSON200 == nil {
		log.Printf("No data found for robot controller %s", controller)
		return
	}

	motionGroups := resp.JSON200.ConnectedMotionGroups
	for _, mg := range motionGroups {
		printMotionGroupPosition(client, cell, controller, mg)
	}
}

func printMotionGroupPosition(client *v2.ClientWithResponses, cell, controller, motionGroup string) {
	resp, err := client.GetMotionGroupStateWithResponse(context.TODO(), cell, controller, motionGroup)
	if err != nil {
		log.Printf("Failed to get motion group %s state: %v", motionGroup, err)
		return
	}
	if resp.JSON200 == nil {
		log.Printf("No data found for motion group %s on controller %s", motionGroup, controller)
		return
	}
	fmt.Println("controller:", controller, "motionGroup:", motionGroup, "joint positions:", resp.JSON200.Positions)
}

func withAuthToken(token string) v2.ClientOption {
	return v2.WithRequestEditorFn(func(ctx context.Context, req *http.Request) error {
		req.Header.Set("Authorization", "Bearer "+token)
		return nil
	})
}
